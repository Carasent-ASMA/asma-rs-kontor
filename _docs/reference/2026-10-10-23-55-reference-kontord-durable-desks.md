# Durable desks in kontord

> Status: ASMA-8450 (TASK-001 of ASMA-8449), merged in PR #295 (`733807cc`,
> 2026-10-11); PR #299 (`813135fd`, ASMA-8454) made a fresh ensure atomic and
> PR #300 (`0971674d`) added seat MCP composition in the desk directory. Merged
> is not deployed: see the [router](../index.md). Describes what the desk
> contract supports at `0971674d`, how a desk-capable Team Definition is
> selected, and how to roll that selection back. Naming rules are in
> [`docs/NATIVE_NAMING.md`](../../docs/NATIVE_NAMING.md#durable-desks-asma-8450).

## What a desk is

A desk is a native project that belongs to a Kontor project, not to an epic:
`DESK • ADAM` and `DESK • PR REVIEW`, each with one `ADAM` workspace. Kontor
stores it as one `desks` row (schema 131) and two topology nodes with no epic
and no task:

- the row plans both node ids. A fresh desk's row and both nodes are written
  in one transaction, so a refused node leaves no row (PR #299); a retry of an
  existing desk reconciles the same nodes rather than placing a second desk;
- the row pins the exact Team Definition revision (id, version, canonical
  hash) that was selected when the desk was first ensured;
- the row is immutable and permanent (database triggers refuse UPDATE and
  DELETE).

The desk's nodes are placed below the project root even when the root was
created under an older topology revision, provided the desk's kinds are
declared, unscoped, by a newer revision of the same topology lineage that the
root's own revision does not declare.

## Supported operations

| Operation | Surface | Effect |
| --- | --- | --- |
| Ensure | `POST /v1/projects/{id}/topology:ensure`, `target: {"scope": "desk", "desk_key": …}` | Desk row and both logical nodes; no native effect |
| Materialize | `topology:materialize`, same target | The above, then the native desk project and its workspace, reconciled by exact native id |
| Drift | `topology:drift`, same target | Exact readback of both natives |
| Read back | `GET topology:inspect` | Both nodes, with `desk_key`; never in an epic-scoped read |

The same operations are served by the MCP topology tools and the CLI generated
from them. Receipts name the project (`ensure_project` for ensure and
materialize, `observe_seat` for drift). A replay with the same idempotency key
returns the original receipt and performs no native effect. The CLI prints the
daemon's refusal body unchanged and exits 3 for `placement_blocked` and
`ensure_mismatch` (PR #299); the full map is in the
[qualification runbook](../operations/2026-10-11-15-28-runbook-durable-desk-qualification.md#supported-operator-operations).

## Support limits

- **No epic, ever.** Native requests for a desk carry no execution scope and
  are never the epic's project container. The node `retire` / `archive` routes
  refuse a desk's nodes with `placement_blocked`, and epic completion,
  closeout and Team Definition migration never select them. There is no
  supported way to retire or delete a desk in this revision.
- **Where a native refusal lands.** The runtime refuses, from the request's
  shape alone and before it asks Paseo anything, a container request or
  inspection that claims to be an epic's project without naming the epic.
  Desk requests never make that claim. Materialize first ensures the desk
  row and both nodes, then prepares the natives from the desk project down,
  binding each before it prepares the next. Materialization is not atomic:
  - a refusal before the first container is bound (that runtime refusal, or
    a failure preparing the desk project) leaves the row and both nodes with
    nothing native bound;
  - a later failure, such as one preparing the workspace, leaves the
    bindings already made in place.

  Either way no receipt is recorded, and a retry with the same key
  reconciles the same two node ids.
- **Declared keys only.** A key that no desk row holds and the project's
  selected Team Definition does not declare is refused (`placement_blocked`)
  by ensure, materialize and drift alike, before any receipt, desk row, node,
  observation or native call. A desk that already exists keeps answering
  after a later selection stops declaring its key (see Rollback).
- **Occupants.** TASK-001 places desks and their workspaces only. Declared
  long-lived desk seats and their missing-only reconciliation are added by
  ASMA-8451; see
  [Durable desk seats in kontord](2026-10-11-02-20-reference-kontord-durable-desk-seats.md).
  Binding an occupant to an epic or a pull request is a later task of
  ASMA-8449.
- **Working directory.** Both of a desk's natives use
  `<runtime root>/<project id>/desk-<desk node id>`, a plain directory rather
  than a Git checkout. When seat MCP composition is configured, a Claude hosted
  seat placed there gets `.mcp.json` and `.claude/settings.local.json` merged
  into that directory, with the originals kept in a private daemon-owned
  journal for rollback and recovery (PR #300). A healthy replay of the same
  native only finishes a pending publication and validates the files; a
  missing or differing required entry is refused without repairing them.
- **Backup.** Export generation 14 carries the `desks` records; older
  generations cannot represent a database that holds desks and are refused for
  one. An import records desk rows as lineage, as it does other topology
  records, and creates no desk in the destination.

## Selecting a desk-capable definition

The bundled default Team Definition declares no desks, and nothing selects a
desk-capable revision implicitly. This build ships one, as additive immutable
successors in the Operational lineages:

| Revision | Version | What it adds to its predecessor |
| --- | --- | --- |
| Operational project session topology | v6 | the `DESK` (parent `PSW`) and `DWS` (parent `DESK`) kinds to v4 |
| ASMA Operational Team Definition | v7 | the `DESK` / `DWS` containers and the `adam` / `pr-review` desks to v1, composed against topology v6 |

The numbers are the lowest free in every known lineage: topology v5 is where
every operator draft of v4 lands, Team Definition v2 and v3 are live, and v4
to v6 are the reserved ASMA-8117 successors. Topology v1/v4 and Team Definition v1 are
byte-identical to the previous build. A project publishes the shipped
revisions the first time it reads its Team Definition; publication skips an
address that already holds a revision.

To use the shipped desks:

1. `team-definition-selection:preview` for `{id, version: 7}`, then `:apply`
   with the returned `preview_hash` and the project revision.
2. `topology:materialize` each desk and confirm the titles and `desk_key`
   through `topology:inspect`.

A project that needs other desks publishes its own successor the same way the
shipped one is built: a topology revision that declares the desk kinds
(`topology-specs:draft`, then `:publish`), then a Team Definition revision
that cites that exact revision and hash with the desk containers and the
`desks` array (`team-definitions:validate`, then `:publish`), then selection
as above.

Selection changes what future epics inherit as well. Existing epics keep
their own pins.

## Rollback

- **Before any desk exists:** select the previous revision again through
  `team-definition-selection:preview` / `:apply`. Published revisions stay
  published; they are immutable.
- **After a desk exists:** selecting the previous revision stops *new* desk
  keys from being ensured, but every existing desk keeps its pinned revision,
  its nodes, its native identities and its names. Its row cannot be removed.
  Leaving desks in place is the supported rollback state.
- **Binary rollback:** a pre-131 build refuses a schema-131 database. Restore
  the snapshot taken before the upgrade, per [`RECOVERY.md`](../../RECOVERY.md).

## Before deploying this build to a realm

Bundled publication is by `(id, version)`: it skips an address that already
holds a revision, and it runs at the start of every read of a project's Team
Definition. Confirm in each realm that the Operational lineages hold no
topology v6 and no Team Definition v7 of their own:

- **Topology v6 occupied:** the shipped v7 cites the shipped v6 by its exact
  hash, so publishing v7 is refused, and with it every read of that project's
  Team Definition until the conflict is resolved.
- **Team Definition v7 occupied:** the shipped v7 is skipped and is not
  available there; selecting `{id, version: 7}` would select the realm's own
  revision. Publish a desk-capable successor through the API instead.
