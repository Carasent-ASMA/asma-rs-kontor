# Plugin catalog consumption, cursors and degradation

> **Date:** 2026-10-25 19:00
> **Status:** implemented on `feat/ASMA-8655-plugin-integration-compatibility-and-degradation`; not merged
> **Category:** architecture
> **Scope:** `/v1/work-catalog` entries/cursors and per-source status, the observer MCP tools, `apps/paseo-plugin` catalog consumption and its versioned snapshot
> **Summary:** TASK-004 makes the shared catalog readable through stable observer tools, pages entries without mixing generations, and lets the plugin prefer the catalog while keeping its direct readers and every operator-owned identity through a daemon outage.

## When to Load

Load when changing the work-catalog entries cursor, the composite source status,
the five observer MCP tools, the plugin's `work-catalog.read` RPC or its
`work-catalog-snapshot.json`. Do not load for the catalog's storage protocol or
the Jira/native collectors; those keep their own notes.

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
`kontor_work_catalog_sources` and follows up to three bounded
`kontor_work_catalog_entries` pages (1,000 rows each). The composite status is the
source of truth for freshness; the entries are mapped to plan summaries by the
preserved operator `source_ids`.

On a successful read the plugin writes its own
`<paseo>/plugin-config/kontor/work-catalog-snapshot.json` atomically. It holds
`schema: kontor/plugin-work-catalog@1`, `version: 1`, `savedAt` (the plugin's
check time) and `sourceAsOf` (the oldest source completion instant in the view).
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
- The plugin does not consume the native inventory in this task, and its entry
  view is bounded; a truncated entry view says so.
- No console TypeScript regeneration: the console does not consume these reads.
