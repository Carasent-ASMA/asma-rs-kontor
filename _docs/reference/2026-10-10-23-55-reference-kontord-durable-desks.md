# Durable desks in kontord

> Status: ASMA-8450 (TASK-001 of ASMA-8449) candidate. Describes what the desk
> contract supports at this revision, how a desk-capable Team Definition is
> selected, and how to roll that selection back. Naming rules are in
> [`docs/NATIVE_NAMING.md`](../../docs/NATIVE_NAMING.md#durable-desks-asma-8450).

## What a desk is

A desk is a native project that belongs to a Kontor project, not to an epic:
`DESK • ADAM` and `DESK • PR REVIEW`, each with one `ADAM` workspace. Kontor
stores it as one `desks` row (schema 131) and two topology nodes with no epic
and no task:

- the row is written first and plans both node ids, so a retry reconciles the
  same nodes rather than placing a second desk;
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
returns the original receipt and performs no native effect.

## Support limits

- **No epic, ever.** Native requests for a desk carry no execution scope and
  are never the epic's project container. The node `retire` / `archive` routes
  refuse a desk's nodes with `placement_blocked`, and epic completion,
  closeout and Team Definition migration never select them. There is no
  supported way to retire or delete a desk in this revision.
- **Declared keys only.** A key the project's selected Team Definition does
  not declare is refused (`placement_blocked`) before any desk row, node or
  native container is written.
- **No occupants yet.** TASK-001 places desks and their workspaces only.
  Hosting seats in a desk workspace, and binding an occupant to an epic or a
  pull request, are later tasks of ASMA-8449; seat launch still requires an
  epic scope.
- **Working directory.** Both of a desk's natives use
  `<runtime root>/<project id>/desk-<desk node id>`.
- **Backup.** Export generation 14 carries the `desks` records; older
  generations cannot represent a database that holds desks and are refused for
  one. An import records desk rows as lineage, as it does other topology
  records, and creates no desk in the destination.

## Selecting a desk-capable definition

The bundled default Team Definition declares no desks, and nothing selects a
desk-capable revision implicitly.

1. Publish the topology successor that declares the `DESK` (parent `PSW`) and
   `DWS` (parent `DESK`) kinds: `topology-specs:draft`, then
   `topology-specs:publish`.
2. Publish the Team Definition successor whose `topology` cites that exact
   revision and hash, with the `DESK` / `DWS` containers and the `desks`
   array: `team-definitions:validate`, then `team-definitions:publish`.
3. Select it: `team-definition-selection:preview` for the exact
   `{id, version}`, then `:apply` with the returned `preview_hash` and the
   project revision.
4. `topology:materialize` each desk and confirm the titles and `desk_key`
   through `topology:inspect`.

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

## Open at this revision

Which lineage versions the shipped desk successors occupy is a release decision
still open with the coordinator: the live Team Definition lineage already holds
versions beyond the bundled default and others are reserved. The tests publish
the successors through the supported surfaces above and depend on no bundled
version number.
