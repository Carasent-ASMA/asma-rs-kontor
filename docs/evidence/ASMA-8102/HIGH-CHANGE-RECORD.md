# ASMA-8102 high-change record: publication-attestation immutability

Date: 2026-09-21

Artifact: `high-change`

Phase: `high-implementation`

Task: `ASMA-8102` / `01a0721b-ea31-7482-952f-f6f004019537`

Epic: `ASMA-8101` / `01a0721b-ea30-7fe3-88a5-4d33ca613414`

TeamRun: `01a075b2-ae1a-7c52-ba3a-b7a372069ea0`

## Result

**PASS for the bounded implementation correction of audit finding
F-8102-A1.** Additive schema migration 119 now refuses every `UPDATE` and
`DELETE` of `publication_attestations` at the SQLite boundary. A current-schema
regression inserts meaningful accepted and refused decisions, proves both
mutation kinds fail for both rows with the explicit trigger refusal, and proves
the original accepted flag, refusal reasons and head SHA remain readable
unchanged.

The complete PUB-01 domain and loopback suites pass at the correction source,
including insert, read, replay, changed-intent conflict, repository scoping,
cross-project/epic/task refusal, mandatory pull-request title refusal and
stale-head refusal. This is producer implementation evidence. It is not an
independent QA or audit verdict, a gate write, a deployment receipt, or a claim
that ASMA-8102 is Done. Fresh QA and audit remain owed after integration.

## Source and audit identity

| Boundary | Exact identity |
| --- | --- |
| Authorized root integration base | `1b524140e83fc9ce48bdf9cab46d8553ecba5c05`, tree `c5ea3516f75d2144e2c33224d016d877f602207c` |
| Corrective implementation source | `f5a365c0aa085dc22b2c2683cbaba16a3dca2f75`, tree `e11c427f26ce35392a2ad5062e9b52dc52bf26de` |
| Root-integrated combined-119 candidate | `4fade7f799a9bb889a0b397588e250b3708cd4f1`, tree `69d7749946c9eddf112eb9726b30e54bf46c35dc` |
| Original PUB-01 corrective source | `bdf088b153652ed02ea03e0589392ceb62e23ced` / pre-rebase `e02845876f8cc200408e9bdd4b0c6394e5ace040` |
| Mandatory-title mutation evidence | `d19cdb3dc552c8d9a77e58ca4c89cfe81f669e1d` (PR 246) |
| Rejected audit turn / artifact | `01a0c358-d0af-7c91-8060-e908c4b99c93` / `01a0c359-3b6a-7c02-9486-3169a6eab912` |
| Audit report | commit `658d5908f96c8b11a28a438669d8441d8c3ea1c4`, `docs/evidence/ASMA-8102/HIGH-AUDIT-REPORT.md`, SHA-256 `279c1aeeeda49b655ec285b61ebae17c25fe65393e4970376ceb0b1c9c8b91aa` |

The audit-report commit is report-only and is not an ancestor of the authorized
integration base. Its exact Git object and supplied SHA-256 were verified from
the common repository; this correction neither copies over nor edits the
auditor's record. The older high-verification pass and the later rejected audit
remain history. The auditor owns its gate record.

The correction commit changes exactly:

```text
A crates/kontor-store/migrations/0119_publication_attestation_immutability.sql
M crates/kontor-store/src/migrations.rs
M crates/kontor-store/tests/schema_v1.rs
```

Migration 0089 was not changed. Its Git blob remains
`dd7d8d23b8fbcc5f2f4ebe85ad810661ad490618`; its SHA-256 remains
`3bc88eacd9d272e8d3cbe46d37c8730c0235aa272b10a192a0841d671194e741`.
Read-only Git comparison after root integration found no difference between
`f5a365c0` and `4fade7f7` in any of the three correction paths; all four blobs
listed below, including the unchanged migration 0089 blob, match exactly.

## Correction

Migration `0119_publication_attestation_immutability.sql` adds two unconditional
database guards:

- `publication_attestations_no_update` aborts every row update; and
- `publication_attestations_no_delete` aborts every row deletion.

Both return `a publication attestation is immutable`. The migration advances
`user_version` to 119. `SCHEMA_VERSION` and the ordered migration registry now
include exactly that additive script; no existing schema script was rewritten.

The regression
`publication_attestations_reject_update_and_delete_from_direct_sql` inserts:

- an accepted revision-4 ASMA-8102 PR decision in
  `Carasent-ASMA/asma-modules` with reasons `[]`; and
- a refused revision-4 decision in `WrongOrg/asma-modules` with
  `repository_mismatch`.

It attempts to rewrite the accepted flag, reasons and head SHA and to delete
each row. All four operations must return the trigger's explicit refusal. A
final ordered read asserts that both original decisions survived byte-for-byte.

Committed source identities:

| Path | Git blob | SHA-256 |
| --- | --- | --- |
| `crates/kontor-store/migrations/0119_publication_attestation_immutability.sql` | `f8e80bd79032f14bc6e6fac26f9ff3d9b4e79683` | `633b3db5e6b0c3b35eca6cc064013609b89fa8d249753152dfa6aa9921b8c9dd` |
| `crates/kontor-store/src/migrations.rs` | `4985ddfc857fe7349d46898f87a2b1fc8695e510` | `3c068929c6cb2916c5e6b21427699f09be048571e7e1a47372320e1043a69790` |
| `crates/kontor-store/tests/schema_v1.rs` | `a12adf6bc70f90a75a6df6b7597fef11876325ed` | `b7896243eebe02eaa3fceec4821702f51fd8c1311f2b542cb746c24b8177e204` |

## Mutation evidence

Both schema guards were independently removed in the isolated implementation
worktree and the exact regression was executed after each change.

### M1 — remove `publication_attestations_no_update`

```text
cargo test -p kontor-store --locked --test schema_v1 \
  publication_attestations_reject_update_and_delete_from_direct_sql -- --exact
result: exit 101; 0 passed, 1 failed
kill: expected an update refusal but SQLite returned 1 changed row
```

### M2 — remove `publication_attestations_no_delete`

```text
cargo test -p kontor-store --locked --test schema_v1 \
  publication_attestations_reject_update_and_delete_from_direct_sql -- --exact
result: exit 101; 0 passed, 1 failed
kill: expected a delete refusal but SQLite returned 1 changed row
```

After restoration, the migration and test returned to SHA-256
`633b3db5e6b0c3b35eca6cc064013609b89fa8d249753152dfa6aa9921b8c9dd`
and `b7896243eebe02eaa3fceec4821702f51fd8c1311f2b542cb746c24b8177e204`.
The same focused test then passed 1/1. No mutant is present in the correction
commit.

This mutation evidence is specific to F-8102-A1. It supplements rather than
relabels the independent mandatory-title and governed-project mutation evidence
at `d19cdb3d`.

## Original acceptance mapping

| PUB-01 / TASK-001 behavior | Executed proof at `f5a365c0` | Result |
| --- | --- | --- |
| Versioned publication identity and stable typed refusals | Complete `kontor-core --test publication_identity` | PASS, 13/13 |
| Immutable issue binding, repository and head-SHA evidence | Schema 119 guards plus accepted/refused direct-SQL regression and both killed mutants | PASS |
| Insert, read, replay and changed-intent conflict | `an_epic_branch_publication_is_attested_recorded_and_replayed` in the complete loopback suite | PASS |
| Refused decisions remain durable | Schema regression retains `repository_mismatch`; `refused_publications_carry_stable_codes_and_are_recorded` and wrong-repository attestation pass | PASS |
| Repository is part of the atomic decision | Domain repository matrix and `a_publication_into_an_unauthorized_repository_is_refused` | PASS |
| Repository authorization is project- and task-scoped | `another_project_does_not_inherit_the_governed_repositories`, module-less refusal and task-narrowing controls | PASS |
| Cross-project, cross-epic and cross-task identity fails closed | Complete domain suite plus ambiguous/cross-project/task loopback controls | PASS |
| A numbered PR requires its bound key in the title | Domain `a_pull_request_must_carry_a_title` and route `a_pull_request_without_a_title_is_refused`; push control remains accepted | PASS with `pr_title_key_missing` |
| Preview, attest and read API contracts | Complete 458-case daemon loopback suite | PASS, 457 passed / 1 intentionally ignored |
| API and MCP contract packages remain current | Complete `kontor-api` and `kontor-mcp` package tests, including committed OpenAPI parity and MCP registry/schema tests | PASS, 35 API + 74 MCP |
| Current-head comparison refuses stale publication SHA | Exact `github_publication` stale-head unit test | PASS, 1/1; downstream consumer only |

The loopback result includes the exact declared repository, cross-project,
cross-epic/task, missing-title, ambiguity, durable refusal, read, replay and
intent-conflict cases. The single ignored case is
`a_configured_jira_boundary_distinguishes_historical_from_native_completion`,
which declares itself superseded by native connector contract tests and is not
a PUB-01 test.

## Commands and actual results

All successful qualification commands ran from the isolated persistent
worktree on committed source `f5a365c0`, with locked dependencies and
`RUSTC_WRAPPER=`.

```text
cargo test -p kontor-store --locked --test schema_v1 \
  publication_attestations_reject_update_and_delete_from_direct_sql -- --exact
result after restoration: exit 0; 1 passed, 0 failed

cargo test -p kontor-store --locked --test schema_v1
result: exit 0; 65 passed, 0 failed

cargo test -p kontor-core --locked --test publication_identity
result: exit 0; 13 passed, 0 failed

cargo test -p kontor-daemon --locked --test loopback_api
result: exit 0; 457 passed, 0 failed, 1 ignored

cargo test -p kontor-daemon --locked \
  github_publication::tests::a_current_github_head_refuses_a_truly_stale_publication_sha \
  -- --exact
result: exit 0; named test 1 passed, 0 failed

cargo test -p kontor-api -p kontor-mcp --locked
result: exit 0; kontor-api 35 passed; kontor-mcp 74 passed; doc tests 0 failed

cargo fmt --all -- --check
result: exit 0

cargo clippy -p kontor-store --all-targets --locked -- -D warnings
result: exit 0
```

One deliberately broad, non-scoped check did not fully pass:

```text
cargo test -p kontor-store --locked
result: exit 101 in repository_roundtrip;
        87 passed, 1 failed in that binary
failure: v115_confirms_exact_local_mutations_with_typed_provenance_and_preserves_history
         reopened at user_version 115 while hosted_seat_role_personas already existed
```

That test and its fixture were not changed by this correction. The single test
was rerun in a disposable detached worktree at exact base
`1b524140e83fc9ce48bdf9cab46d8553ecba5c05` and reproduced with exit 101,
0 passed / 1 failed and the same
`table hosted_seat_role_personas already exists` error. The disposable worktree
was removed. This is a pre-existing base fixture defect, not evidence against
schema 119 or PUB-01; it is disclosed here and was not repaired outside the
authorized scope.

## Remaining boundaries and handoff

- Root has integrated the exact correction source as `4fade7f7`; only this
  producer-owned evidence commit remains to be integrated. No root integration
  source was mutated from this worktree.
- Fresh independent QA and audit must assess the integrated candidate and own
  any later gate record. This producer makes no independent-review claim.
- Migration 119 is release-pending code. No daemon, database or environment was
  deployed or migrated by this work.
- The previous audit rejection remains append-only. No gate, Jira issue,
  topology, scheduler, watchdog, TeamRun, workspace, seat, run or native
  identity was changed.
- PR #195's forge poller/merge gateway and forge-side bypass controls remain the
  separately bounded PUB-02/deployment concern. The stale-head test is retained
  only because the original TASK-001 acceptance and this qualification request
  name that downstream refusal.
- ASMA-8234 content in the integration base was not used as ASMA-8102 scope and
  no ASMA-8234 file was edited.
- The task remains open. Open questions: none; F-8102-A1's required behavior and
  the pre-existing unrelated fixture failure are both evidenced above.
