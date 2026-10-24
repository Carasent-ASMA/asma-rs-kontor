# Prototype source binding

This app imports the current plugin client directly. It has no plugin mirror.
The cumulative source is `042a120462909b5f0bda7699104676a45e54ba0b`, root
`8d10d0e467ef606579ecd45e2e97233a729d6461`, plugin subtree
`5bf39b737d3fa199430ca9833ec6e1a08e0ab279` (257 files).

The existing PR460 branch retains its published `ac134d94` ancestor. Integration
resolves every production path to that exact cumulative source; the resulting
PR diff contains only this app and selective root-lock additions. No old plugin
pin or old lockfile is transplanted. Existing lock importers, packages and
snapshots remain unchanged. The app adds one importer and 21 missing web package
and snapshot records.

## Scope and fixture boundaries

All 31 scenario IDs survive the earlier prototype. The entry calls the real
plugin contribution and renders its actual client through React Native Web.
Host discovery, storage, navigation, sources, lifecycle and New LSA responses
are fictional page-memory adapters. Unknown APIs fail visibly. No real account,
host, native session, workspace or provider is contacted.

`sources.summaries` returns the same catalog identity as `sources.list`, preserves
Jira/plan metadata, and sets content, graph and records to null. Sessions needs
only summaries. Opening a selected Plan demands that exact full source; the
browser proof rejects full reads of unselected sources in that check.

The lifecycle adapter accepts only selected archive batches of 1–50 IDs. An
omitted active child blocks a parent; selected children must finish first.
Synthetic archive membership and runtime retirement remain separate. An
archived/unconfirmed reply retains its exact-owner fence through later successful
reviews and Resume. The no-ID latest read derives unresolved references; reads
of a named review do not. Reconciliation reads exact IDs without redispatching.
These fixtures exercise current UI behavior, not every production server race,
durable filesystem or provider failure. Reloading the page resets the simulated
server; React remounts retain that page's fixture receipts.

The model fixture retains unknown, zero, invalid, cache and context-only cases,
and adds a synthetic routed-model tuple and K/M report. The recovery proof covers
the notice icon, archived sessions having no Open action, unchanged Refresh
status, and per-item Hide acknowledgement. No real Refresh or lifecycle action
is exercised.

## Verification

From the candidate repository root:

```sh
git diff --exit-code 042a120462909b5f0bda7699104676a45e54ba0b HEAD -- apps/paseo-plugin
git diff --exit-code 042a120462909b5f0bda7699104676a45e54ba0b -- apps/paseo-plugin
pnpm --filter @carasent/kontor-paseo-plugin-prototype typecheck
pnpm --filter @carasent/kontor-paseo-plugin-prototype test
pnpm --filter @carasent/kontor-paseo-plugin-prototype build
```

The browser checker starts a fresh isolated profile against a temporary loopback
preview. It permits only same-origin GETs and blocks WebSockets. It checks the
31 scenarios, desktop/320/390 layouts, profile/sort, synthetic creation, metadata
source demand, selected child-first bulk archive and exact-owner recovery.
Screenshots, command results, source-map content parity and hashes are retained
in operator-local evidence, outside Git.

No prototype installation or promotion is part of this change. A new repository
release gate, independent review, native Dynamic Type, physical touch, assistive
technology and real lifecycle acceptance are not claimed. MAIN and TEST releases
and their settings are untouched by this source-only work.
