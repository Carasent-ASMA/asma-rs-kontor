Artifact: implement-to-verify-handoff

# ASMA-8116 implement-to-verify handoff

## Head

| Field | Value |
| --- | --- |
| Branch | `feat/ASMA-8116-implement-confirmed-jira-key-resolution-and-public-projections` |
| Head | `8c87e4e` |
| Integrated onto | `origin/master` `e40b5f4` (contained; verified ancestor) |
| Worktree | `/Users/igor/carasent/asma-modules/.worktrees/asma-8116/asma-rs-kontor` |
| Schema | `SCHEMA_VERSION = 95`, migration `0095_immutable_jira_issue_identity.sql` |
| Recovery ref | tag `asma8116-preintegration-2-backup` → `b0b711f` |

Commits above the integrated master:

```
8c87e4e chore(kontor): integrate origin/master and record the ASMA-8116 placement gap
18b6990 WIP(ASMA-8116): rename/anti-rebind coverage and daemon fixtures
3e01adf WIP(ASMA-8116): immutable Jira issue id through connector, ledgers and reconciliation
e7fad76 WIP(ASMA-8116): migration 0094 immutable jira issue identity  (file now 0095)
77f8366 WIP(ASMA-8116): provisional scope-turn working tree
```

The earlier partial-admission correction is **not** carried here: it landed
upstream as `e7a760e` (#207) and its replay was dropped as a duplicate. The
deployed upstream copy carries the tightened predicate
(`binding_id.is_some() && native_id.is_some()`), which is the correct one.

## Delivered against the frozen scope

- **Immutable Jira issue identity (clauses 1–4).** `0095` adds a nullable
  `external_issue_id` to `jira_epic_bindings` and
  `jira_task_binding_confirmations`, unique per project, with triggers making an
  established id immutable. Pre-migration rows keep NULL; none is synthesized,
  and such a row stays fail-closed for key changes. The connector retains Jira's
  REST top-level `id` alongside the key, and the existing readback hash is
  unchanged so already-confirmed rows stay replayable.
- **Resolver and refusals (clauses 5–7).** `resolve_confirmed_jira_key` resolves
  project-scoped exact keys across both confirmed ledgers to the immutable
  Kontor subject UUID. `reconcile_confirmed_jira_key` performs a same-issue
  rename addressed *by immutable id*, preserving subject UUID, link identity and
  revision; a different id is a typed anti-rebind refusal; a legacy row without
  an id cannot authorize a rename. Uniqueness violations map to stable typed
  conflicts rather than raw `rusqlite` errors.
- **v81 trigger narrowed (OQ-003).** `canonical_jira_task_links_immutable` is
  replaced by an identity-immutability trigger plus a key-change trigger that
  admits a rename only when the confirmation ledger already holds an immutable
  id *and* the link ledger already names the new key. Deletes stay blocked.

## Exact commands and outcomes (2026-09-12, head `8c87e4e`)

| Command | Outcome |
| --- | --- |
| `git rebase origin/master` | rebased; 3 conflicts resolved (`graph.rs`, migration renumber, loopback EOF) |
| `cargo check -p kontor-core -p kontor-store -p kontor-api -p kontor-daemon` | clean |
| `cargo test -p kontor-core --test backlog_identity` | ok, 8/8 |
| `cargo test -p kontor-store --test jira_materialization` | ok, 14/14 |
| `cargo test -p kontor-store --test jira_link_ledger` | ok, 5/5 |
| `cargo test -p kontor-api` | ok, 23/23 + 5/5 + OpenAPI contract 3/3 |
| `cargo test -p kontor-daemon --test loopback_api jira` | ok, 9 passed, 1 pre-existing ignored |
| `cargo test -p kontor-daemon --test loopback_api a_partially_seated_candidate` | ok, 1/1 |
| `cargo test -p kontor-daemon --test mcp_journey` | **FAILED**, 1 passed / 1 failed — see Blocker 1 |
| `cargo test -p kontor-store --test schema_v1` (with `0095`) | run 1 FAILED, run 2 ok 57/57 |
| `cargo test -p kontor-store --test schema_v1` (without `0095`) | ok, 57/57 |

The OpenAPI contract test passing at 3/3 means the checked-in contract and the
generated console schema survived the merge unchanged; no regeneration was
required.

## Mutation evidence

| Mutation | Result |
| --- | --- |
| Disable the shared cross-subject guard in `confirm_jira_materialization_item` | killed — `activation_requires_every_confirmed_binding_and_survives_readback` fails at its typed-conflict assertion |
| Neuter the blocked path (`&blocked` → `&[]`) in `mark_started_tasks_in_progress` | killed — the partial case fails `ready` vs `in_progress`; this also reproduced the reported bug |
| Weaken the progress predicate to `binding_id.is_some()` only | killed — the unattached case fails `in_progress` vs `ready` |

No mutation was retained.

## Blockers

### Blocker 1 — placement requires a legacy backlog code (OQ-004, open)

`an_empty_realm_is_bootstrapped_through_mcp_tools_alone` fails with
`placement_blocked: the epic has no active immutable backlog code`. Clause 9
leaves epics without an active code; placement still requires one. Two
reconciliations were attempted and **both withdrawn** on instruction —
reinstating allocation (violates the no-allocation fence) and carrying the
confirmed key through `JiraItemCode` (changes legacy `ITEM_CODE` semantics).
Both are fully reverted; `backlog_identity.rs` and its tests are byte-identical
to `origin/master`.

Settling it needs the approved plan's specification for `EPIC_JIRA_KEY`,
`TASK_JIRA_KEY` and `SCOPE_JIRA_KEY`: extending the published `NativeNameToken`
closed contract and its `spec.rs` allow-list, a migration changing which seeded
templates are new-key-only, and the legacy-consumer boundary per call site. The
binder call sites are already lazy, so no code change is needed to make them
conditional — the gap is the missing tokens and the seeded templates that name
item codes. Full detail in `OPEN-QUESTIONS.md` OQ-004.

### Blocker 2 — `schema_v1` concurrent-open intermittency (OQ-002, open)

`a_concurrent_first_open_initializes_exactly_one_realm` fails intermittently
under the full target. The paired comparison did not attribute it: the same arm
produced a failure and then a pass. Settling it needs repeated runs on both arms
and a comparison of failure *rates*. Single-run evidence must not be used to
blame or clear `0095`.

### Blocker 3 — control plane unavailable to this seat

The Kontor MCP server failed to connect this session (`CONNECTION_CLOSED`), so
no TeamRun dispatch, session message or runtime settle could be recorded from
here. This document is the durable artifact; the dispatch identity must be
issued by Kontor. No session, workspace, topology, TeamRun, AgentRun or
Jira/GitHub identity was created, replaced or altered.

## Remaining scope, not delivered

- Focused migration/export/backup/recovery and concurrent-race tests for the
  v95 upgrade path (scope item 5) — the rename, anti-rebind, fail-closed-legacy,
  direct-SQL, replay and restart cases are covered; a v94→v95 upgrade-preserving
  test and an explicit race test are not.
- Clause 9's public-projection consequences, which are gated on Blocker 1.
