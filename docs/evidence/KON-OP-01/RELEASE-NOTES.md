# KON-OP-01 / ASMA-7870 — release notes

Date: 2026-08-16
Commits: `dedd300`, `597fa26`
Schema: 24 → **25**

## Summary

Published Kontor topology specifications and role catalogs now carry an
immutable write-time shareability classification: whether the document may ever
leave Kontor, who classified it, and whether that came from the type default or
a human override.

This records eligibility only. Nothing is published anywhere. There is no
publisher, synchronization, file writer, inbound import, conflict resolver or
drift detector in this release.

## Schema migration

`0025_document_shareability.sql` adds three columns to `topology_specs` and
`role_catalog_revisions`, plus one insert trigger per table.

**Upgrade is automatic and in place.** An existing realm opens unchanged: every
already-published document adopts the tier-B default `project_shared` with
provenance `type_default` and no classifier, because no human classified it. No
operator action is required and nothing is prompted for.

**Downgrade is not supported**, consistent with every prior Kontor migration: a
newer schema is refused rather than truncated.

## Behaviour changes

- `publish_topology_spec` and `publish_role_catalog` require a classification.
  Both refuse a stamp whose classifier identity and provenance disagree, and
  refuse a non-default class presented as though the default rule produced it.
- `get_topology_spec_shareability` and `get_role_catalog_shareability` are new
  read ports. Existing document readers are unchanged.
- Backup export carries the classification beside the document and its hash;
  restore preserves it.
- Canonical document hashes are **unchanged**. Classification is stored beside
  the document, so no pinned `(spec_id, revision, canonical_hash)` moved.

## Not in this release

- Any publication, synchronization or repository-writing behaviour.
- Classification of tier-A operational state — seats, bindings, receipts,
  scheduler/capacity state, provider/model routing and cost, unapproved memory,
  per-run scratch context. These refuse classification by construction and have
  no column to hold one.
- `/v1`, MCP, CLI or UI exposure of the classification. Projection is OP-03;
  the diagnostic surface is OP-09.
- `independent_review@1` and `operational_default@1`, which the plan assigns to
  OP-05 and OP-06.

## Risk

Low. Additive columns with defaults, additive read ports, and one changed
argument list on two publish ports whose only callers are OP-01's own tests.
Full workspace suite green at 1264 tests; clippy clean at `-D warnings`.
