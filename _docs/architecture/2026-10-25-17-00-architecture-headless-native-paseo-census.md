# Headless native Paseo census

> **Date:** 2026-10-25 17:00
> **Status:** implemented on `feat/ASMA-8654-headless-native-paseo-inventory`; not merged
> **Category:** architecture
> **Scope:** `crates/kontor-runtime-paseo/src/census.rs`, `crates/kontor-catalog` native tables and routes, `kontord` composition
> **Summary:** A read-only, UI-free census of every Paseo project, workspace and session, archived sessions included, persisted with its own generation, page checkpoints and fallback snapshot.

## When to Load

Load when changing the native census transport, the native `catalog.db`
tables, generation/fencing or archive coverage, the
`/v1/work-catalog/native/*` reads, or how `kontord` composes the census
connection. Do not load for the frozen plan-discovery contract
([the v1 contract](2026-10-25-11-00-architecture-shared-observational-catalog-contract.md))
or for plugin consumption, which TASK-004 owns.

## What it observes

One dedicated, read-only connection per distinct Paseo endpoint:

| Read | Native operation | Scope |
| --- | --- | --- |
| Runtime identity | pushed `status/server_info` | version, boot id, permissions, features |
| Projects | `project.list.request` | every project, including one with no active workspace |
| Workspaces | `fetch_workspaces_request` with no project filter | every workspace, with project metadata and `emptyProjects` |
| Sessions | `fetch_agents_request` with an empty label filter and `includeArchived: true` | every session page, archived included |

The census does not run the Paseo CLI and sends no mutating request. It does
not reuse the governance adapter's project bindings: a session outside every
governed project is in scope.

## Identity

A native identity is the pair `(runtime_instance, native_id)`, where
`runtime_instance` is `paseo:<endpoint>` and `native_id` is the daemon's
project, workspace or session id. A title, a display name and a working
directory are data, never keys. Two sessions with one title remain two rows;
two projects with one display name remain two rows.

The runtime instance is the endpoint, not the daemon's per-boot `serverId`.
Native ids survive a restart, so keying observations by boot would create a
second identity after every restart. The boot id and application version are
stored as provenance.

The endpoint is not a credential. The complete `--host` target is held in a
secret wrapper by the shared transport and never reaches this module's
payloads, storage, snapshot, logs or errors. Checkout remote URLs are
deliberately not stored.

## Connection identity and reconciliation

The census transport is constructed with `clientId = <plane-client-id>-census`.
That is distinct from the governance adapter's identity and from the ZeroSub
poller's, so a reconnect by one never moves another's resumable session.

Each attempt:

1. discards any previous socket (`reset_connection`), so a connection that
   survived a sleep and now silently times out is not reused;
2. re-gates on a fresh pushed identity and refuses a daemon below the supported
   version baseline or without `workspace.read`;
3. walks every page from the first cursor;
4. re-reads the boot identity and discards the walk if it changed, because
   pages read across two daemons are not one directory.

A partial walk is an incomplete observation. It is never a complete empty
directory.

## Pagination and archive coverage

The walk follows at most 1,000 pages of 200 rows, the same bound the plugin's
lifecycle guard uses. `hasMore` requires a non-empty, previously unseen cursor;
a missing or repeated cursor makes the attempt incomplete instead of
truncating the list silently. Page counts are recorded per runtime.

Session pages request `includeArchived: true`. Archive scope is reported as
covered only when the whole walk completed; an incomplete walk reports it as
not covered. Each session row keeps its `archivedAt` stamp, and status counts
active and archived sessions separately.

## Storage and freshness

Migration `0003_native_census.sql` adds a separate native lifecycle to the
catalog database:

- `catalog_scope.native_generation` and `native_active_generation`, distinct
  from the plan generation, so a Paseo failure cannot make plan discovery look
  stale and a plan sweep cannot advance native freshness;
- `catalog_native_sweeps` and `catalog_native_sweep_pages`, the attempt and its
  atomic page checkpoints;
- `catalog_native_projects`, `catalog_native_workspaces` and
  `catalog_native_sessions`, keyed by runtime instance and native id;
- `catalog_native_collection`, the latest attempt, its retry instant and
  per-runtime coverage.

The protocol is the plan protocol applied to native observations:

- a page commits its observations and its page checkpoint in one transaction;
- only a finished walk that read every configured runtime promotes pages,
  advances `native_active_generation` and tombstones what it proved absent;
- an incomplete walk may insert a previously unknown row hidden
  (`visible_generation IS NULL`) and may not update or remove an existing row;
- a page from a superseded generation or owner is fenced and commits nothing;
- removals are rows with `state = 'removed'`, retained as history, never
  physical deletion;
- a runtime that disappeared from configuration is not absence: only runtime
  instances present in the completed walk may have rows tombstoned.

The native fallback snapshot is `<state-root>/catalog-native-snapshot.json`.
It is written atomically after a complete walk and records the generation's
completion instant as `written_at`. A read that falls back to it reports that
instant as `as_of`; request time is never presented as source freshness. An
incompatible snapshot version is refused.

Native absence is observation only. It never completes a task, changes a plan,
enrolls a project, admits work or mutates a session.

## Reads

Both routes require the `observer` tier and the realm credential, like the
plan reads:

| Route | Answer |
| --- | --- |
| `GET /v1/work-catalog/native/status` | Native generation, attempted generation, as-of, collector, retry instant, per-runtime coverage, counts and snapshot state |
| `GET /v1/work-catalog/native/inventory?include_removed=&instance=&limit=` | One generation-consistent native inventory for projects, workspaces and sessions |

`include_removed` exposes observations removed at the active generation.
`instance` selects one runtime instance. `limit` is a hard bound per
collection; `next_cursor` continues the same generation and is refused when it
moved. The plugin projection is documented in
[`2026-10-25-19-00-architecture-plugin-catalog-consumption.md`](2026-10-25-19-00-architecture-plugin-catalog-consumption.md).

## Composition

`kontord` reads the same `<state-root>/runtimes.json` it already uses for the
governance adapters. Each distinct Paseo endpoint becomes one census source;
a second plane on the same endpoint is the same instance and is deduplicated.
An absent or uncomposable source is logged and skipped: the native census is
observational, so it never refuses governed startup. `kontor-daemon` runs the
native poller beside, not inside, plan discovery at the configured catalog
cadence, sharing the collector lease but not the generation.

## Limits in this task

- No remote transport, no multi-host inventory and no speculative second
  runtime adapter.
- No session mutation, governance enrollment, scheduler action or upstream
  write.
- No labels, prompts, transcript content or remote URLs are stored.
- No plugin/MCP consumption and no console TypeScript regeneration; TASK-004
  owns the plugin projection and cursors. The four observer-tier catalog reads
  are recorded in the MCP parity allowlist, so no Lead tool is invented ahead
  of that task.
- Absence after a complete walk is retained as a tombstone; it is not a
  delivery or task outcome.

## Verification

- `crates/kontor-runtime-paseo/tests/census.rs`: protocol/targeted pages,
  missing and repeated cursors, page bound, permission and version refusal,
  unreachable runtime, boot-identity change, connection reset on start and
  failure, and a read-only live preflight that is `#[ignore]` by default.
- `crates/kontor-catalog/tests/native_census.rs`: cold start with archived
  sessions and duplicate titles, bounded checkpoint pages, incomplete walks
  that delete nothing, unavailable runtime, reconnect completion, stale page
  fencing and fallback freshness.
- `crates/kontor-daemon/tests/work_catalog_api.rs`: authenticated native reads
  through the real router, with plan freshness unchanged.
- `crates/kontor-daemon/src/runtimes.rs`: the census client identity is
  distinct and endpoint-scoped.

A complete local census is bounded at 30 seconds; the fixture suites assert
the bound, and the read-only live preflight measured a full host inventory in
under two seconds. The exact live receipt belongs to the task handoff, not to
this document.
