# ASMA-8116 migration sequencing constraint

## Directive

Recorded during the ASMA-8116 implementation turn. This is sequencing, not a
scope change; the admitted contract in `HIGH-SCOPE-RECORD.md` is unchanged.

Current `origin/master` (`40ad3a5`) owns schema **v93**
(`0093_consultation_session_releases.sql`). ASMA-8117 is further through
validation and is required to rebase and merge **first**, taking **v94** as
`0094_consultation_subject.sql`.

ASMA-8116 therefore must not finalize or merge while it holds a conflicting
`0094`.

## Obligation before final gates

1. Wait for ASMA-8117 to land as v94 on master.
2. Rebase this same branch/worktree
   (`feat/ASMA-8116-implement-confirmed-jira-key-resolution-and-public-projections`)
   onto the integrated master.
3. Renumber the immutable Jira issue-identity migration
   `0094_immutable_jira_issue_identity.sql` → `0095_...`, and update every
   schema reference with it: the `include_str!` entry in
   `crates/kontor-store/src/migrations.rs`, `SCHEMA_VERSION` (94 → 95), the
   script's own trailing `PRAGMA user_version`, and the `assert_eq!(SCHEMA_VERSION, …)`
   narrative in `crates/kontor-store/tests/schema_v1.rs`.
4. Preserve both v93 consultation-release behavior and v94 consultation-subject
   behavior. Neither is superseded by this work.

## Why the renumber cannot be done early

The migration mechanism is an ordered `include_str!` array dispatched on
`user_version`: the script at array position *i* must set `user_version = i + 1`.
Naming this script `0095` before ASMA-8117's `0094` exists in the array would
leave position 94 holding a script that claims version 95, desynchronizing the
dispatch. The renumber is therefore correct only *after* the rebase that
introduces `0094_consultation_subject.sql`.

Until then the branch deliberately carries `0094_immutable_jira_issue_identity.sql`
so that local verification remains runnable.
