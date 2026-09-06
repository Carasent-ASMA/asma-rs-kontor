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
