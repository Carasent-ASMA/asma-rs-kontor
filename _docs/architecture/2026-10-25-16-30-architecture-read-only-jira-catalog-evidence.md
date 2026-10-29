# Read-only Jira catalog evidence (v1)

> **Date:** 2026-10-25 16:30
> **Status:** implemented on `feat/ASMA-8653-read-only-live-jira-discovery-and-reconciliation`; not merged
> **Category:** architecture
> **Scope:** `crates/kontor-jira/src/read_only.rs`, `crates/kontor-catalog/src/jira*.rs`, catalog schema v2 and the `kontor/epic-identity@1` projection
> **Summary:** TASK-002 adds a GET-only Jira observation path with its own scope, generation and coverage, and projects it to the frozen epic-identity view without changing the catalog's v1 association authority.

## When to Load

Load when changing the read-only Jira seam, Jira observation storage, the
`kontor/epic-identity@1` projection, or the `jira` section of `catalog.json`.
Do not load for the governed write connector (`crates/kontor-jira/src/connector.rs`),
`kontord`'s reconciliation barrier, or the plugin's immediate export script.

## Two Jira paths, one credential, different authority

| | Governed connector | Catalog read-only seam |
| --- | --- | --- |
| Construction | Private, per Kontor project | Explicit scope, one configured project |
| Operations | Observe, dry-run, apply transitions/fields/assignees | GET only |
| Configuration | `<state-root>/jira.json` projects | `catalog.json` `jira` section plus the matching `jira.json` endpoint/alias |
| Authority | May write upstream when `reconcile_epic_keys` confirms an epic | Never writes upstream |
| Lifecycle | Resident reconciliation barrier | Its own Jira generation and checkpoint; never waits on the barrier |

Both resolve the keychain entry through `JiraCredentialScope` and the same
strict credential document, refuse redirects instead of following them, and
bound every response and request with the same connect/request budget. The
catalog seam exposes its own `JiraReadOnly` and `JiraReadFailure`; it does not
make the governed connector's private authenticated operation public.

## The observation rules

- **Declared keys are read directly.** A scope search cannot prove a key the
  operator named. A direct answer is an issue, a move (old key to current key)
  or a 404 (`missing`), never an assumption.
- **Jira-only epics are discovered.** The configured scope is the discovery
  path, so a new Epic with no catalog membership or `ws:` label is observed.
- **Stories stay Stories.** The actual issue type, hierarchy level and parent
  come from Jira. A `ws:<key>` label on a Story does not make it an Epic.
- **The configured scope is explicit.** `scope.jql` is sent verbatim (bounded
  to 2,000 characters). The collector additionally requires every included
  issue to belong to `scope.project`.
- **Parents climb from Jira**, through current keys and old-to-current aliases,
  for a bounded number of rounds. An unread parent is `unresolved`, not an
  inferred epic.
- **A verified join is separate evidence.** `verified_workstream_join` requires
  an actual Epic at hierarchy level 1, exactly one non-empty `ws:<key>` label,
  and a plan/registry declaration of that key. The catalog's frozen
  `Association::verified` remains `false`; this evidence type never rewrites it.
- **Moved keys are aliases.** A direct read of an old key that Jira answers with
  a new current key records `moved_key` with the current key and retains the old
  key as an alias.

## Pagination, overlap and recovery

- Enhanced search (`/rest/api/3/search/jql`) is walked to `isLast`; a repeated
  page token is refused, and page/issue bounds stop a runaway logical search.
- A full sweep is a complete relevant census. An incremental sweep adds an
  overlapping `updated >=` window (the configured overlap subtracted from the
  last complete instant) and never removes a fact.
- A periodic full sweep replaces incremental windows at
  `fullSweepSeconds`. Coverage exposes `lastFullSweepAt`,
  `nextFullSweepAt` and `overdue`; a late full sweep reports `overdue` instead
  of pretending it was on time.
- 429 and 5xx answers retry a bounded number of times. `Retry-After` is
  honoured; a value beyond the seam's cap is surfaced as an absolute
  `backoffUntil` so the collector cadence, not a hidden sleep, owns it.
- A failed page, a 404, a rate limit or permission loss is uncertainty. It
  retains the prior fact and makes coverage say so.
- Only a complete **full** sweep may tombstone. Previously observed issues are
  rechecked directly; an issue that still matches the configured scope after a
  search page omitted it is retained and `searchLagSuspected` is set. A 404 is
  `retainedUnknown`. A verified issue that no longer matches the relevant set
  is tombstoned with `outOfScope`.
- A stale answer (an older Jira `updated` instant) may prove presence but may
  not overwrite newer fields. A page or finish from a superseded generation is
  fenced.
- Removed `ws:` labels are observed and diagnosed; no association is deleted.
  Tombstones, aliases and dangling explicit catalog associations remain
  readable.

## Storage

Catalog schema v2 adds `catalog_jira_scope`, `catalog_jira_sweeps`,
`catalog_jira_sweep_pages`, `catalog_jira_observations`,
`catalog_jira_aliases` and `catalog_jira_collection`. It does not alter any v1
table. Jira pages stage observations; promotion is one transaction; an
incomplete or failed sweep inserts only previously unknown rows, hidden.

The durable evidence document is `kontor/jira-observation@1` and carries:

- `source` (realm, base URL), `scope` (JQL, project, in-project declared keys),
  `provenance` (collector, adapter, credential reference only);
- `coverage` (scope/declared/found/missing/out-of-scope/unresolved, pages,
  requests, retries, rate limits, permission/credential loss, search lag,
  retained unknowns, removals, window, schedule and diagnostics);
- issues (immutable id, current key, actual type/level, parent, labels, status,
  category, updated, summary, observed_at, via), aliases, moved-key conflicts,
  dangling associations, `latestError` and `unanswered`.

The frozen compatibility view is `kontor/epic-identity@1`, pinned as
`crates/kontor-catalog/tests/fixtures/epic-identity-v1.schema.json` (SHA-256
`913055c2343608afed3e88a014434bf2237daa46d3c94f67d00af6b51741f9c7`). Its
projection refuses duplicate issue keys, a moved-key conflict whose current key
is not in the document, and any declared key that is not answered by an issue,
a move or `missing`.

## Configuration

`catalog.json` gains one optional section. A present section defaults to
enabled; absent means no Jira collection.

```json
{
  "schemaVersion": 1,
  "jira": {
    "enabled": true,
    "project": "ASMA",
    "scopeJql": "project = ASMA AND issuetype = Epic",
    "declaredKeys": ["ASMA-1000"],
    "endpoint": "https://tracker.example",
    "credentialAlias": "work",
    "cadenceSeconds": 120,
    "fullSweepSeconds": 900,
    "overlapSeconds": 180,
    "maxPages": 200,
    "maxIssues": 5000,
    "maxRecheck": 500,
    "pageSize": 100,
    "maxAttempts": 3,
    "scopeIsEpics": true,
    "parentRounds": 5
  }
}
```

`endpoint` and `credentialAlias` may be omitted; the matching project in
`<state-root>/jira.json` then supplies them. No credential value enters
`catalog.json`, diagnostics, logs or the evidence document. A composition
failure warns and leaves governed service untouched; an absent section starts
no Jira poller.

## Deliberate limits

- No upstream write, transition, assignment or JQL mutation.
- No governed reconciliation barrier, no scheduler admission and no enrollment.
- No second immediate collector or export script; the plugin's current script
  remains the temporary immediate path, and this is the durable path/view.
- No Paseo session inventory; configured declarations are the declaration
  source in this task, and the projection reports zero session records.
- Search consistency lag prevents a universal new-issue discovery bound. The
  declared keys and the periodic full sweep are the provable coverage.
