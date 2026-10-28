# Plugin catalog consumption, cursors and degradation

> **Date:** 2026-10-25 19:00
> **Status:** implemented on `feat/ASMA-8655-plugin-integration-compatibility-and-degradation`; not merged
> **Category:** architecture
> **Scope:** `/v1/work-catalog` entries/cursors and per-source status, the observer MCP tools, `apps/paseo-plugin` catalog consumption and its versioned snapshot
> **Summary:** TASK-004 makes the shared catalog readable through stable observer tools, pages entries and native inventory without mixing generations, and lets the plugin use the catalog as a per-id fallback while keeping its direct readers, exact native identity and every operator-owned association through a daemon outage.

## When to Load

Load when changing the work-catalog entries cursor, the composite source status,
the five observer MCP tools, the plugin's `work-catalog.read` RPC, the shared
native-inventory adapter or its `work-catalog-snapshot.json`. Do not load for the
catalog's storage protocol or the Jira/native collectors; those keep their own
notes.

## Cursors are generation-bound

`EntriesView.next_cursor` is opaque and carries the generation, the row offset
and a fingerprint of the filter (`kind`, `scope`, `include_removed`). Presenting
it after the active generation moved returns `stale_cursor` (410): a page may not
be assembled from two sweeps. The same cursor with another filter returns
`invalid_request` (400). A native inventory cursor carries one offset per
collection and follows the same rule. Limits are clamped: 1,000 entries per page
and 10,000 native rows per collection.

The entries answer also names its read source. A fallback snapshot answers with
its own generation and `as_of`, and a cursor from another generation is refused,
so a snapshot cannot silently continue a database page.

## Per-source status

`GET /v1/work-catalog/sources` returns plan, Jira and native rows in that order.
Each row carries:

- `state` (`not_configured`, `never_collected`, `complete`, `incomplete`,
  `failed`), `generation` and `attempted_generation`;
- `as_of`, the completion instant of the last complete generation — never the
  request instant and never the instant the status was read;
- the latest structural `error` and the source's own `retry_at` (a plan attempt's
  next cadence instant, Jira's `Retry-After` deadline, or the native retry
  instant);
- bounded `coverage` lines (repository/runtime/scope counts, archive coverage,
  sweep schedule and overdue state);
- bounded `conflicts` (plan identity/content conflicts and Jira moved keys);
- `pending_verification`, the active plan associations the frozen v1 contract
  cannot verify. Jira's missing or unresolved declared keys are named in its
  coverage line instead, because they are a coverage gap, not an association. A
  source that cannot be read is a failed row, not a failed answer.

## Fixed observer tools

Five observer-tier reads are fixed registry rows and are served by the `worker`
and `leadership` profiles; consultation profiles do not reach them:

| Tool | Route |
| --- | --- |
| `kontor_work_catalog_status` | `GET /v1/work-catalog/status` |
| `kontor_work_catalog_sources` | `GET /v1/work-catalog/sources` |
| `kontor_work_catalog_entries` | `GET /v1/work-catalog/entries` |
| `kontor_work_catalog_native_status` | `GET /v1/work-catalog/native/status` |
| `kontor_work_catalog_native_inventory` | `GET /v1/work-catalog/native/inventory` |

They are reads with no idempotency header, they mirror the daemon's observer
tier, and the MCP parity oracle now requires each to have a tool rather than an
allowlist entry.

## Plugin consumption and fallback

`apps/paseo-plugin/server/work-catalog.ts` uses the same observer boundary as
`kontor-status.ts`: it starts `kontor-mcp` with `--state-root`,
`--credential-tier observer` and the optional serve profile, gives the child only
`PATH` and `HOME`, and never opens, names or forwards a credential. It asks for
`kontor_work_catalog_sources`, `kontor_work_catalog_native_status`, up to six
bounded `kontor_work_catalog_native_inventory` pages and up to three bounded
`kontor_work_catalog_entries` pages (1,000 rows each). The composite status is the
source of truth for freshness; the entries are mapped to plan summaries by the
preserved operator `source_ids`. The plan pages are read before the optional
native inventory so a slow native tool cannot delay or hide them.

## Native inventory is selected by exact host identity

`work-catalog.read` takes `hostId`, the exact Paseo `serverId` of the plugin
host. It is never a title or label. The server reads the native status and keeps
the coverage row whose `server_id` equals `hostId`; exactly one such row must
exist, with a positive generation and a completion instant. A missing, ambiguous
or incomplete match is an unavailable native read, not a best-effort title match.

The native inventory request pins the exact runtime instance and asks for up to
six pages of 1,000 rows with `include_removed: false`. The 6,000-row native view
bound covers the known 5,654-record production census. The daemon orders active
sessions before archived ones, so an archived census that outgrows the bound
cannot push active sessions out of the read. Every page
must carry the same runtime instance, generation, `as_of`, source and archive
scope. Rows are deduplicated by native id. `truncated`, a missing completion
instant, or an archived request against `archiveScope: false` is a visible
partial, never an exact empty host. A native view that is still larger than the
bound stays visibly partial rather than silently reporting a complete total.

`client/shared-inventory.ts` is the one adapter at the screen composition point.
It keeps a ready, live, complete direct directory ahead of the catalog, maps
native projects, workspaces and sessions to the existing directory shapes from
the catalog only when direct cannot answer, and overlays live
labels, pending permissions, needs-input, attention, active-turn, telemetry and
availability only by exact session id. A `catalog`-mode answer whose native view
source is `snapshot` is the daemon's own fallback, not a complete live catalog:
a usable direct directory wins, and the daemon snapshot is used only when direct
cannot answer. A complete exact-empty catalog is empty; it is not treated as
unavailable. If the catalog cannot answer, a usable direct directory takes over;
a partial catalog remains visible with its marker; the last-known native
snapshot is used only when neither live source can answer. A snapshot keeps its
saved native `asOf`, never the plugin check time, and its pill/read label says
last-known rather than complete live.

Sessions tree/list/groups, archive filters, project and context selectors,
Start/switch, Overview, AI teams, Search, Activity/Inbox, Settings associations
and Board/Graph all read that adapted result. Native actions (send, stop, open
agent or workspace) stay direct Paseo operations through the plugin host; the
adapter never invents an action path or writes Paseo.

## Snapshot version and host binding

On a successful read the plugin writes its own
`<paseo>/plugin-config/kontor/work-catalog-snapshot.json` atomically. Version 2
(`schema: kontor/plugin-work-catalog@2`, `version: 2`) adds `hostId`, the runtime
instance, `nativeAsOf` and the native inventory to the plan view. It is bound to
that host: a different host refuses it rather than reading another host's rows.
A v1 snapshot remains readable for plan summaries and carries no native
inventory, so it can never be mistaken for native data. A later native mapping
failure keeps the previously stored native view instead of replacing it with a
partial file.

When the observer is absent, restarting or incompatible, the RPC answers
`mode: snapshot` with that view and the source `asOf`, or `mode: unavailable`
with no view. It never answers an empty catalog for an unreadable one.

The plugin never opens `catalog.db`. Exact Markdown, graph export
(`asma plan export`), Teams snapshots and every current direct reader stay in the
plugin and remain the path for source content. A snapshot or the direct readers
never write the source catalog, pins, source IDs, favorites, explicit operator
mappings or settings.

The Overview page's Sources and coverage section shows the mode, each source's
`asOf`, coverage, error, retry deadline, conflicts and pending verification. A
last-known snapshot is labelled as such; the plugin's `savedAt`/check time is
never labelled as source freshness.

## ASMA-8646 compatibility

The plugin consumes the reviewed ASMA-8646 shapes rather than creating a second
association source: a captured epic may carry `ws`, the Jira projection keeps it,
the registry accepts `match`/`paseo_project` on a desk, and the shared
`WORKSTREAM_CONFLICT` diagnostic spelling is used. The catalog's own
`Association::verified` remains `false`; `pending_verification` exposes that
honest limit instead of turning a declaration into a verification.

## Limits

- No catalog or plugin write to Jira, Paseo, plans, settings or the governed
  store; no session mutation and no enrollment.
- The plugin's native inventory is bounded to six pages / 6,000 rows; the daemon orders
  active sessions first, and a larger or archive-incomplete inventory stays
  visibly partial.
- Direct rows win per exact id; catalog rows fill only the ids direct does not
  have. A ready, live, complete direct directory answers alone, and the catalog
  answers only for ids direct cannot supply, including an exact empty host.
- The console TypeScript schema is regenerated when the served contract changes;
  these reads are not yet consumed by a console screen.
