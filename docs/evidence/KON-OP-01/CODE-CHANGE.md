# KON-OP-01 / ASMA-7870 — code change

Date: 2026-08-16
Branch: `feat/ASMA-7870-kontor-operational-domain`
Frozen submodule: `_tools/asma-rs-kontor`
Commits: `dedd300` (shareability), `597fa26` (mutation-driven test),
`181c628` (master merge), `b367683` (ECP + code help)
Predecessor baseline: `7314721` generic topology domain, on the deployed hotfix `6b3e95c`

## What this delivers

Two amendments layered onto the predecessor's generic topology domain:

1. **OP-REQ-037** write-time `shareability` for the published documents OP-01
   owns (`dedd300`, `597fa26`).
2. **OP-REQ-040/041** the Epic Control Plane and server-owned code help
   (`b367683`), from the 2026-08-16 13:02 amendment.

The predecessor run landed the generic topology domain at `7314721` (08:19).
Each amendment postdates it, which is why each arrived as a separate gap rather
than as part of the original build. `181c628` merges master's OP-REQ-039
seat-attachment guardrails in between.

## Domain — `crates/kontor-core/src/spec.rs`

| Item | Line | Purpose |
| --- | --- | --- |
| `ShareabilityTier` | `:136` | Closed three-tier vocabulary. The tier is a property of the record *type*, so it is a constructor argument, never a stored field that could drift from the row it describes. |
| `ShareabilityTier::default_class` | `:165` | Tier A refuses; tier B defaults `project_shared`; tier C defaults `kontor_local`. |
| `ShareabilityClass` | `:179` | `project_shared \| kontor_local`. |
| `ShareabilityProvenance` | `:189` | `type_default \| human_override`. |
| `ShareabilityClassifier` | `:200` | The default rule, or the named human who overrode it. |
| `Shareability` | `:225` | The stamp: class, classifier identity, provenance. |
| `Shareability::default_for` | `:239` | The ordinary path — nobody is asked. |
| `Shareability::overridden_by` | `:252` | A human override; the identity is mandatory, not optional. |
| `Shareability::validate_for` | `:272` | Refuses tier A, a classifier that disagrees with its provenance, and a `type_default` stamp whose class is not the tier default. |

## Ports — `crates/kontor-core/src/repository.rs`

`publish_topology_spec` and `publish_role_catalog` now take `&Shareability`.
`get_topology_spec_shareability` and `get_role_catalog_shareability` read it
back. The read ports are additive, so no existing caller of `get_topology_spec`
or `get_role_catalog` changed.

## Schema — `crates/kontor-store/migrations/0025_document_shareability.sql`

Next free number after the integrated Foundation branch (`0024`), per OP-CON-002.
`SCHEMA_VERSION` 24 → 25.

Three columns on `topology_specs` and `role_catalog_revisions`. The tier-A tables
from `0023` — `topology_nodes`, `seat_bindings`, `adaptive_admission_state` — are
untouched: refusing classification means having nowhere to put one, not storing
a null.

The columns carry defaults, so an existing realm opens unchanged and every
already-published document reads back as `project_shared` by the type-default
rule. One `BEFORE INSERT` trigger per table enforces the classifier/provenance
pairing that `ALTER TABLE` cannot express as a table-level `CHECK`. Both tables
already refuse `UPDATE`/`DELETE` through the `0023` triggers, so the stamp
inherits that immutability.

## Store — `crates/kontor-store/src/repository.rs`

| Item | Line |
| --- | --- |
| `TOPOLOGY_SPEC_TIER` / `ROLE_CATALOG_TIER` | `:907`, `:910` |
| `stored_shareability` — re-proves the pairing on read as well as write | `:916` |
| `publish_topology_spec` / `get_topology_spec_shareability` | `:983`, `:1019` |
| `publish_role_catalog` / `get_role_catalog_shareability` | `:1201`, `:1233` |

## Export — `crates/kontor-store/src/backup/export.rs`

The three columns join the typed `topology_specs` and `role_catalog_revisions`
export rows, so restore preserves the classification alongside the document and
hash.

## Design decision: the stamp sits beside the document, not inside it

The canonical hash keeps identifying the specification text alone. Had the stamp
been folded into the canonicalized document, the same specification withheld
versus shared would produce two different hashes, and every epic that pinned
`(spec_id, revision, canonical_hash)` would be sensitive to a classification
decision that says nothing about the topology. This also leaves every existing
document hash and the predecessor's fixtures byte-identical.

## ECP and server-owned code help (`b367683`)

Added under the 2026-08-16 13:02 amendment (OP-REQ-040/041, DEC-033/034).

**OP-REQ-040 — Epic Control Plane.** The seeded vocabulary is now
`PSW QSW ESW ECP TSW ASW CSW`. `LSA` and `TPM` are removed as node kinds: they
were never containers, and modelling them as kinds gave each control role its
own workspace. One `ECP` sits under each `ESW` with cardinality exactly one,
`native_child + session_host` and never `native_root`, so the default claims no
nested Paseo workspace. Both control seats bind directly to that one ECP node.

**OP-REQ-041 — code help.** `CodeHelp { full_name, meaning, category, lifecycle }`
hangs off every declared node kind; roles carry the same five facts through
`role_code`, `standard_title`, `responsibility_summary`, `segment` and a new
`lifecycle`, rather than a parallel structure that would duplicate title and
category. `historical_codes` explains codes a client meets in old state but must
never create — `TSC` (compatibility) and `PASE` (retired) — because a vocabulary
describing only its current kinds would force every client to hard-code the
rest, which is the competing dictionary the requirement forbids.
`ProjectSessionTopologySpec::code_help` resolves both lists and returns `None`
for an unknown code rather than guessing.

Validation ties the two together: a declared kind must be `current`, a
`historical_code` must not be, and no code may be both.

Role meanings now come from the approved dictionary instead of echoing each
title back.

**Why this is seed data, not a migration.** The fixture has never been published
by production code — `publish_topology_spec` and `publish_role_catalog` have no
callers outside `kontor-profiles`' seed and OP-01's own tests, verified before
the change. So no stored document, pinned hash, running topology or bound seat is
affected, which is what the amending brief's constraint 4 required.

## Scope

Every changed file is inside the OP-01 ownership list. No `/v1`, MCP, CLI,
promotion, Jira/memory, publication surface, repository writer, synchronization
or drift detector was added — the MVP stores and exposes the classification
only, and the ECP/code-help change is seed data with no runtime projection. No
Foundation fixture or snapshot was modified, and no topology, workspace, TeamRun
or seat was created or migrated. `independent_review@1` and
`operational_default@1` were **not** seeded — the amended plan does assign them
to OP-01, but this run was scoped to shareability, ECP and code help; see
`OPEN-QUESTIONS.md` OQ-OP-01-1.
