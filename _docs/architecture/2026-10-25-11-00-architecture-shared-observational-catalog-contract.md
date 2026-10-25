# Shared observational work catalog — frozen contract (v1)

> **Date:** 2026-10-25 11:00
> **Status:** implemented on `feat/ASMA-8652-catalog-storage-identity-local-discovery-and-kontord-hosting`; not merged
> **Category:** architecture
> **Scope:** `crates/kontor-catalog`, its `catalog.db`, local plan discovery, `/v1/work-catalog`, and `kontord` hosting
> **Summary:** The public types, storage protocol, configuration precedence and read boundary of the host-local work catalog. TASK-001 freezes this contract; later tasks extend collection and consumption without changing its meaning.

## When to Load

Load when changing a catalog type, the `catalog.db` schema, identity or
association rules, configuration precedence, the collector's
generation/checkpoint protocol, or the `/v1/work-catalog` reads. Do not load for
the model/work-profile catalog served at `/v1/catalog`; that is governed
application data with its own contract.

## What the catalog owns

One separable module, `crates/kontor-catalog`, owns:

- `<state-root>/catalog.db` — its own SQLite file, schema version 1, independent
  of `kontor.db` (currently schema 136);
- `<state-root>/catalog.json` — the operator-owned effective configuration;
- `<state-root>/catalog-snapshot.json` — an atomic fallback snapshot;
- the collector lease, sweep generations, page checkpoints and tombstones.

It shares no table, no transaction and no scheduler with governed execution.
Catalog startup does not require an enrolled project, a scheduler admission, a
governed `kontor.db`, or a running plugin UI. Zero configured repositories is a
valid catalog and reports a complete sweep with zero coverage.

## The public contract

The Rust types in `crates/kontor-catalog/src/contract.rs` are the contract.
`CATALOG_CONTRACT_VERSION = 1` is carried on every status and entries body. A
field may change only with a contract-version change; storage migrations add
columns behind these types.

### Three identities, never collapsed

| Identity | Meaning | Stability |
| --- | --- | --- |
| `logical_plan_id` | The plan people mean | Stable across renames when exactly one absent predecessor matches the same repository, content revision and declared epic |
| `occurrence_id` | One exact file in one checkout or worktree | Stable for one `(repository, root, path, content kind)` |
| `revision_id` | SHA-256 of the exact bytes | Content identity |

An explicit operator `logicalPlanId` is preserved and wins. Front matter may
declare `catalog_id` or `plan_id`; both spellings must agree and match
`^[a-z][a-z0-9_-]{0,63}$`. Duplicate explicit ids remain visible and produce a
`duplicate_explicit_id` conflict; neither silently wins.

A single unambiguous rename retains the old logical identity and records the old
path as a `renamed_from` alias. Two or more candidates produce an
`ambiguous_rename` conflict and no alias. The same logical plan in two worktrees
is one entry with two occurrences; different revisions produce a
`divergent_worktree` conflict.

### Associations are not verifications

`Association::kind` is `explicit_operator` or `plan_declaration`. The operator's
explicit choice is retained when it disagrees with a plan declaration; the plan
declaration stays visible and an `association_conflict` is recorded.

`Association::verified` is always `false` in contract version 1. TASK-001 has no
Jira observation, so no plan/epic or `ws:<key>` join may claim verification. A
later collector must add its own evidence contract rather than flip this flag.
Unknown, absent and conflicting registry keys remain diagnostics and never
create registry entries.

### Selection and coverage

| Content | Included | Meaning |
| --- | --- | --- |
| Tracked Markdown under `_docs/`, `plan/`, `plans/` | yes | Working copy |
| Non-ignored untracked plan Markdown | yes | Saved draft, visibly distinguished |
| `.github/plans` | only when the configured root sets `includeGitHubPlans` | Explicitly permitted |
| `.github/plans/qnr` | with the same permission | `kind = execution_unit`, never a plan or epic |
| Ignored, symlinked, non-plan-shaped or over 2 MiB | no | A structural diagnostic, never content |

A file under `_docs` is plan-shaped when it carries a `plan-graph` fence or has a
plan-shaped path/name, mirroring the existing direct reader. Non-epic and
malformed declarations leave the document visible with diagnostics.

## Storage protocol

- **One owner per scope.** A live lease on `catalog_scope` admits one collector;
  a competing owner is refused before any scan (`owner_conflict`).
- **Pages are evidence, not truth.** A scan commits bounded pages. A page commit
  writes its observations and page checkpoint in one transaction.
- **Only a finished sweep promotes.** The final transaction of a sweep that
  attempted every configured root promotes staged observations, advances
  `active_generation`, and tombstones what that sweep proved absent.
- **Partial scans never delete.** An incomplete sweep may insert a previously
  unknown row, hidden (`visible_generation IS NULL`), and may not update or
  remove an existing row. Reads are generation-consistent: plans and occurrences
  are only visible at `active_generation`.
- **Failed roots never delete.** Tombstoning is restricted to repository ids
  whose complete contents were read in the finished sweep. A failed root keeps
  every fact it last supplied; coverage reports the failure.
- **Stale work is fenced.** A page write from a superseded generation or owner
  commits zero rows and returns `fenced`.
- **History is retained.** Removed plans and occurrences keep their rows,
  aliases and associations. Tombstones are never physical deletion.
- **Associations survive.** Explicit operator associations attached to a
  removed plan become `dangling`; they are reported in status and never
  rewritten away.
- **The snapshot is a fallback, not an authority.** It is written atomically
  (temporary file, `fsync`, rename) after a finished sweep and is read only when
  the database read fails. An incompatible snapshot contract version is refused.

## HTTP read boundary

Both routes require the `observer` tier at minimum; one authenticated realm
credential is still required.

| Route | Answer |
| --- | --- |
| `GET /v1/work-catalog/status` | Freshness (`generation`, `as_of`), collector ownership, coverage, counts, registry projection, diagnostics, conflicts, dangling associations, snapshot state |
| `GET /v1/work-catalog/entries?kind=&scope=&include_removed=&limit=` | One generation-consistent entries view. `scope` matches a repository id, worktree root, path prefix, logical plan id or workstream key |

`/v1/work-catalog` is deliberately distinct from `/v1/catalog`, which serves the
governed model/work-profile catalog. TASK-004 owns MCP tools, pagination cursors,
plugin consumption and its versioned snapshot; this task exposes no cursor and
no plugin surface.

## Configuration precedence and migration

The catalog resolves the **effective** plugin configuration once, in the
plugin's existing order:

1. `KONTOR_PLUGIN_READ_SOURCES` when non-empty; otherwise
   `<paseo>/plugin-config/kontor/read-sources.json`.
2. `KONTOR_PLUGIN_ASMA_WORKSPACE`; otherwise
   `<paseo>/plugin-config/kontor/registry.json`'s `workspaceRoot`; the registry
   is `<workspace>/config/orchestration/workstreams.json`.

`<paseo>` is `PASEO_HOME`, otherwise `$HOME/.paseo`. When anything resolves, the
catalog materializes `<state-root>/catalog.json` atomically and records its
origin and revision in `catalog_config`. From then on `catalog.json` is the one
operator answer and wins over the environment and plugin files — because
`launchd` does not inherit a terminal environment, and a daemon must not change
scope silently when an operator's shell does.

`catalog.json` (camelCase, schema version 1) holds:

```json
{
  "schemaVersion": 1,
  "repositories": [{"root": "/absolute/checkout", "includeGitHubPlans": false}],
  "sources": [{"id": "operator-id", "kind": "plan", "path": "/absolute/plan.md",
               "workstream": "kontor", "epic": "ASMA-1000",
               "revisionKind": "working copy", "expectedSha256": null,
               "logicalPlanId": null}],
  "registryPath": "/absolute/config/orchestration/workstreams.json",
  "cadenceSeconds": 60,
  "scanBudgetSeconds": 30
}
```

Source IDs, expected hashes, workstream/epic declarations, `revisionKind` and
`logicalPlanId` are preserved from the plugin source catalog. An
`approved revision` requires a pin. Invalid values are dropped with a
diagnostic; they never become a second authority.

## Deliberate limits in TASK-001

- No Jira, Paseo, network or upstream write.
- No `asma` child process; exact-source graph export stays in the plugin.
- No filesystem watch. Polling at the configured cadence is the completeness
  proof.
- No registry entry creation, governance enrollment, pin activation, session
  mutation or plugin/MCP change.
- No standalone/multi-host SQLite and no second background reconciler in
  TypeScript.

## Related

- Plan: `asma-modules/_docs/ai-orchestration/plans/2026-10-25-02-28-plan-shared-observational-catalog.md`
- Existing direct reader: `apps/paseo-plugin/server/read-sources.ts`, `plan-discovery.ts`
- State-root inventory: [`docs/CONFIGURATION.md`](../../docs/CONFIGURATION.md#where-configuration-lives)
