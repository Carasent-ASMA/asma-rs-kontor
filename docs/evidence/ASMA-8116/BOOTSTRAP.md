# ASMA-8116 Jira-key admission bootstrap

Date: 2026-09-06

## Scope

This is the bounded bootstrap needed to admit ASMA-8116 and ASMA-8117. It is
not completion evidence for either task's resolver, public-projection, typed
Jira-token, migration, or rollout scope.

The live scheduler refused both ready tasks before any TeamRun existed because
`execution_scope` required a durable task short code. Both tasks deliberately
have no legacy code under the approved Jira-key policy. The approved gap memory
at revision `01a07757-417e-7241-a890-0fd185a1621d` records the failed supported
message path and the bounded implementation fallback.

Fresh supported readback at cursor 2309 recorded:

- realm `01a00649-9ee6-73e0-ba1b-6a6c35cfd065`;
- project `01a0064a-e056-7603-9968-ef64fdaacb75`, revision 6;
- epic `01a0539a-51c9-7301-9bd7-26c09167b23e`, revision 1;
- ASMA-8116 task `01a07722-c34b-74f3-9880-c3361ff88021` and ASMA-8117 task
  `01a07722-c376-77f2-bee9-609793e172de`, both ready, confirmed Jira-linked,
  with `short_code: null` and no TeamRuns;
- active unbound TSW nodes `01a0772f-d6f2-7302-8e8c-69bdf3e68f61` and
  `01a0772f-d7a1-7701-845a-136bd0611bc8`.

No legacy mapping, Jira mutation, topology replacement, or task lifecycle
change is part of this source correction.

## Correction

- `TaskScope.short_code` is optional and explicitly compatibility-only.
- A task under a durable typed epic execution scope may materialize without a
  short code. A legacy/untyped import still receives the existing explicit
  mapping refusal before runtime contact.
- Legacy `KONTOR_BACKLOG_CODE` and `<short ticket code>` inputs are supplied
  only when a historical code exists. A Jira key is never copied into that
  field as a substitute.
- Team Definition rendering and consultation semantic validation resolve an
  old item code only when the pinned template actually uses an old item-code
  token. Existing token meanings and pinned definition hashes are unchanged.
- Configured compatibility planes continue to provide their historical code as
  an explicit `Some(...)` value.

The focused loopback regression
`a_typed_task_without_a_legacy_short_code_materializes_without_inventing_one`
confirms a typed Jira-linked task materializes and reads back with
`short_code: null`. The existing
`a_legacy_jira_import_materializes_semantic_epic_control_and_ticket_titles`
test confirms the untyped legacy path still refuses inference and proceeds only
after the supported explicit mapping.

## Local verification

All commands ran in the exact ASMA-8116 isolated worktree on base
`508a5141e0ecab8b207e1b7112f839e3f1bb9a2d`.

| Check | Result |
| --- | --- |
| `cargo check --workspace --all-targets` | PASS |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| Typed null-short-code loopback regression | PASS, 1/1 |
| Existing legacy mapping/recovery loopback regression | PASS, 1/1 |
| Runtime, Paseo, Codex, AO and Team package suites | PASS; only their declared live-environment cases ignored |
| `git diff --check` | PASS |

The full `cargo test --workspace` run passed the complete daemon loopback slice
(300 passed, one predeclared ignored test), including both regressions, plus the
Jira materialization, backlog identity, runtime, scheduler, backup and export
suites reached before one unrelated store stress test returned SQLite
`DatabaseBusy`:
`schema_v1::a_concurrent_first_open_initializes_exactly_one_realm`. No changed
file belongs to that store/schema path. The exact failing test then passed four
consecutive isolated runs. This record deliberately does not label that whole
workspace invocation green.

Deployment and post-restart scheduler receipts belong in a separate live
receipt after the exact reviewed commit is installed.

## First deployment and canonical checkout refusal

The first bootstrap correction was committed as
`e0f46c1bf3ec541f1217b770c469ebbfc4008420`, reviewed in PR 203, attested by
ASMA receipt `01a0777f-d67d-7002-ba13-73472341fde3`, and merged to
`c4a0e6bd462a9880f45982a8058907b931b810c2`. The feature and merge trees are
identical at `7d9a7ce0537677726636955008a6c256b30795ea`.

The exact merge was built in a detached checkout and deployed to the complete
local runtime fleet. The atomic backup and deployment receipt is:

`/Users/igor/.local/state/kontor/asma/deploy-backups/20260906T161929Z-asma-8116-c4a0e6b/deployment.json`

Post-restart readback preserved realm, schema 92, the project and epic IDs,
the ESW/ECP identities, both TSW node IDs, all pinned Team Definition data,
and the Paseo project/workspace inventory. SQLite integrity was `ok` and the
foreign-key check returned zero rows. The installed daemon SHA-256 was
`2ccc0a7ec55bb5d698572bfc1bd6260f5a5f4972afe144f1da7d451f590cc50d`.

Scheduler plan `d53d0e3eb06862802bd697130c394c809495ffdd34fc1b2f1fbf5df13a4608b9`
was started exactly once with idempotency key
`asma-8049-jira-key-bootstrap-retry-20260906-1`. It started no TeamRuns and
reported both tasks `placement_blocked`. The daemon log recorded the exact
refusal `rule=the branch-encoded worktree belongs to another Git repository`.
This was a safe pre-launch refusal: the two TSW nodes remained active and
unbound, and no native identity was replaced.

The originally declared paths encoded a feature branch directly under
`.worktrees/`. The supported ASMA catalog shape for a module checkout is
`.worktrees/<jira-key-lowercase>/asma-rs-kontor`. Identity-preserving Git
worktree moves established:

- `/Users/igor/carasent/asma-modules/.worktrees/asma-8116/asma-rs-kontor`;
- `/Users/igor/carasent/asma-modules/.worktrees/asma-8117/asma-rs-kontor`.

The ASMA-8116 branch, HEAD and tree stayed unchanged and clean after the move;
the ASMA-8117 branch stayed at its original clean `508a514` source state.

## Catalog worktree compatibility correction

A no-write whole-epic preview first replayed an unrelated historical worktree
and refused its old `kbi-01` branch form. A second no-write preview specified
only the two paths being corrected and exposed the remaining bootstrap defect:
the daemon rejected the canonical catalog slug `asma-8116` as
`branch_type_unknown` before reaching its existing Git common-directory and
branch-binding checks. Neither preview changed stored state.

`kontor-core` already defines `managed_catalog_worktree_parts` and documents
the two-stage caller contract: parse a branch-encoded worktree first, then
recognize the ASMA catalog module shape. `place_task_branch` now implements
that contract. For a catalog path it requires a confirmed Jira epic/task key
and accepts only an exact lowercase slug match. An unconfirmed binding returns
the typed binding-unconfirmed refusal, and a foreign catalog slug returns the
typed binding-mismatch refusal. Existing branch-token semantics and the later
repository/module/actual-branch verification remain unchanged.

Two focused regressions prove acceptance of the exact catalog shape and
refusal of a foreign Jira-key slug. The complete daemon loopback suite passed
302 tests with zero failures and one predeclared ignored live case, including
the earlier null-short-code bootstrap, branch grammar, worktree, scheduler,
topology and replay coverage. Fresh-target `cargo check --workspace
--all-targets`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo fmt --all -- --check`, and `git diff --check` all passed. A first check
attempt after the Git worktree move found only a stale Tauri cache file whose
generated path referenced the old checkout; a fresh external target proved
the source and all targets clean.

This follow-up remains bounded to admission compatibility. Stored worktree
paths will be updated only through the supported whole-epic preview/apply
surface after this exact correction is reviewed, merged, and deployed.
