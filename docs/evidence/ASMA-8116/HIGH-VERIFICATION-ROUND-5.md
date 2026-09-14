# ASMA-8116 high-verification report — round 5

Date: 2026-09-12
Artifact: `high-verification-report-round-5`
Task: `ASMA-8116` / `01a07722-c34b-74f3-9880-c3361ff88021`
Phase: `high-verification`
Candidate: `170c0b02e5ec9510baaa6bc4b75bef30514a04e2`
Candidate tree: `34ac7322a0c369bd9e63b567fd1b2f251604f825`
Candidate parent: `527decf58cfcd68ad69f60dad5a5d30a3551ac83`
Repair account: `GATE-4-REPAIR-REPORT.md`
Status: **PASS — no release-blocking finding remains at the exact candidate**

## Verdict

The three round-4 rejection findings are closed at
`170c0b02e5ec9510baaa6bc4b75bef30514a04e2`. Both persisted rename counters are
non-decreasing at the SQLite boundary; repository and reconciliation failures
retain the required permanent/transient classification through HTTP; and a
content-conflict DTO is built from the current post-observation projection.

Independent mutation runs killed M11, M12 and M13 for the intended reason. The
mutants were applied only to an archive of the exact candidate under
`/private/tmp`; each source was restored byte-identically. Candidate production
sources were not edited during verification. No Jira, topology, PR, deployment
or other external state was changed.

The result is **PASS**. The known OQ-002 and OQ-004 conditions and the 0095
integration collision remain accurately attributed below; none is newly caused
or silently settled by this repair.

## Exact candidate boundary

- `HEAD` and the configured upstream both resolved to
  `170c0b02e5ec9510baaa6bc4b75bef30514a04e2` before the report was added.
- The branch was
  `feat/ASMA-8116-implement-confirmed-jira-key-resolution-and-public-projections`
  and its candidate worktree was clean.
- Review covered the complete 9-file, 571-insertion / 14-deletion repair delta
  from `527decf58cfcd68ad69f60dad5a5d30a3551ac83`.
- No ASMA-8117 implementation or consultation-subject file appears in that
  delta. The ASMA-8116 migration remains internally numbered 0095; final
  collision resolution remains an integration responsibility.

## Round-4 finding disposition

| Round-4 finding | Round-5 disposition |
| --- | --- |
| sequence rewind can reactivate occurrence authority | **closed** for task and epic by storage-level non-decreasing guards, retained A→B→C→A/rewind/replay regressions and M11 |
| repository and durable reconcile failures collapse to transient | **closed** by typed refusal paths, exact HTTP assertions and M12 |
| content conflict reports the superseded link key | **closed** by using the rebuilt projection, a first-pass assertion, zero-extra-read comparison and M13 |

### Storage-monotonic task and epic occurrence counters

Migration 0095 installs `BEFORE UPDATE OF rename_sequence` triggers on both
`jira_task_binding_confirmations` and `jira_epic_bindings`, aborting only when
`NEW.rename_sequence < OLD.rename_sequence`. The focused migration suite proves
both trigger definitions survive the v94→v95 path.

The retained task and epic regressions each perform A→B→C→A through supported
same-issue reconciliation, refuse a raw rewind to zero, allow a forward repair
advance, then prove the original A→B authority remains spent. Both passed in the
30-test store suite.

### Failure type and HTTP boundary

`confirmed_jira_identities` read failure returns `IdentityRefusal::Transient`.
After identity has been proved, only `RepositoryError::Backend` from ledger
reconciliation is transient; every durable repository refusal becomes
`IdentityRefusal::RenameRefused`.

The public mapping is preserved and independently exercised:

- `DifferentIssue` → HTTP 409 `stale_binding`;
- `RenameRefused` → HTTP 409 `stale_binding`, with a distinct durable-refusal
  rule;
- `UnprovenIdentity` → `placement_blocked`;
- connector/repository `Transient` → `unavailable`.

The durable-refusal regression also asserts that the binding does not move and
the Jira mutation count remains zero. The four resident-reconciler tests passed
with valid workflow selectors, including task and epic different-ID paths that
offer a real transition yet record zero external mutations. This preserves the
round-4 M8/M9 write-boundary result; the repair delta does not revert those
selectors or the terminal control flow.

### Current-key projection and zero-extra-read behavior

Task identity is decided before policy. On a same-ID rename, downstream plan
state is rebuilt under `current_key` while reusing the original observation.
`TicketContentConflictDto.external_issue_key` comes from that rebuilt
projection rather than the stale link row.

The retained first-pass regression distinguishes the two sources: the link row
begins at `ASMA-1`, Jira reports the same immutable issue at `MOVED-1`, the DTO
reports `MOVED-1`, and the durable binding moves in that pass. Its read counter
then proves the rename pass uses exactly the same number of Jira reads as the
steady pass. The preview/apply regression continues to refuse a stale preview
without an old-key effect and proves a fresh plan writes only to `MOVED-1`.

## Independent mutation record

The mutation workspace was an archive of the exact candidate, not the real
worktree. Every command below ran the named retained test with `--exact`.

| Mutant | Exact mutation and observed kill | Result |
| --- | --- | --- |
| M11 | removed only the task `rename_sequence` monotonic trigger; `a_rewound_rename_sequence_cannot_reactivate_spent_task_authority` failed at the required rewind-refusal assertion | **killed, 0/1 passed** |
| M12 | mapped every reconciliation error to `Transient`; `a_ledger_refused_rename_is_permanent_while_only_outages_are_transient` observed HTTP 503 `unavailable` instead of required 409 `stale_binding` | **killed, 0/1 passed** |
| M13 | sourced the conflict key from `link.external_issue_key`; `a_same_issue_rename_reports_its_content_conflict_against_the_current_key` observed `ASMA-1` instead of required `MOVED-1` | **killed, 0/1 passed** |

Restoration hashes:

| Source | Exact-candidate and restored SHA-256 |
| --- | --- |
| `crates/kontor-store/migrations/0095_immutable_jira_issue_identity.sql` | `c2fe860c8808a77f81fae4d0819d1874dc467491295a2f68d66463866b3114b2` |
| `crates/kontor-daemon/src/applications.rs` | `770fe61b447af2fabe31e715274d88b7416534eb3315fc2f896328ef8e7a8cbe` |

`git diff --no-index` between each restored isolated file and its real
exact-candidate counterpart was empty. Mutation score for this repair is
**3/3 killed (100%)**.

## Independent gate and test record

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | passed |
| focused clippy, four affected crates, all targets, `-D warnings` | passed |
| `cargo test -p kontor-store --test jira_materialization` | passed, 30/30 |
| `cargo test -p kontor-store --test schema_v1` | passed, 58/58 |
| `cargo test -p kontor-jira` | passed, unit 9/9 + native connector 19/19 |
| `cargo test -p kontor-api` | passed, library 23/23 + error contract 5/5 + OpenAPI 3/3 |
| daemon `loopback_api` Jira filter | passed, 9; 1 predeclared ignored |
| daemon `the_resident_reconciler` filter | passed, 4/4 |
| exact restored M12 and M13 regressions | passed, 1/1 each |
| console API verification | passed |
| console typecheck | passed |
| console tests | inherited failure, 299/300: `DeepSeek V4 Flash` expectation versus `DeepSeek V4.1 Flash`; files are outside this candidate delta |
| exact MCP bootstrap journey | expected OQ-004 failure: `placement_blocked`, no seat started |

The isolated `schema_v1` pass includes the concurrent-first-open test. It is one
green observation, not evidence that settles OQ-002's recorded intermittent
failure rate or attribution.

## Preserved contracts and attribution

- Legacy `JiraItemCode`, `NativeNameToken` and hash semantics are unchanged.
  `crates/kontor-core/src/backlog_identity.rs` and
  `crates/kontor-core/src/naming.rs` are byte-identical to `origin/master`.
- OQ-002 remains open and correctly characterized as intermittent; this round's
  58/58 run neither blames nor clears migration 0095.
- OQ-004 remains an operational placement/template gap. The exact journey fails
  before any seat starts; no legacy code was minted and no token contract was
  altered to hide it.
- `origin/master` already owns 0094. ASMA-8116 owns its branch-local
  `0095_immutable_jira_issue_identity.sql`, while ASMA-8117 independently owns
  `0095_consultation_subject.sql` on its branch. No ASMA-8117 file was edited;
  final numbering is still assigned to integration.
- OQ-005 remains the historical chain-of-custody record for the interrupted
  round-4 verifier turn at `527decf`. `HIGH-AUDIT-REPORT.md` durably records the
  independently reproduced rejection; this round does not rewrite that history.

## Non-blocking hygiene note

`git diff --check 527decf..170c0b0` reports one added blank line at end of
`HIGH-AUDIT-REPORT.md`. It does not affect source, schema or runtime behavior
and is not a release-blocking finding, but the repair delta is not literally
whitespace-clean.

No release-blocking finding remains at
`170c0b02e5ec9510baaa6bc4b75bef30514a04e2`.
