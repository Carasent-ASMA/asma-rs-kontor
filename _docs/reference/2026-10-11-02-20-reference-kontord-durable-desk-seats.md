# Durable desk seats in kontord

> Status: ASMA-8451 (TASK-002 of ASMA-8449), merged in PR #296 (`430975a3`,
> 2026-10-11); merged is not deployed, see the [router](../index.md). Describes how a
> long-lived seat of a durable desk is declared, how the daemon reports it, and
> the one reconciliation it supports: restarting a selected, daemon-owned seat
> that the runtime has proved missing. Desks themselves are described in
> [Durable desks in kontord](2026-10-10-23-55-reference-kontord-durable-desks.md).
> Seat titles are described in
> [`docs/NATIVE_NAMING.md`](../../docs/NATIVE_NAMING.md#durable-desk-seats-asma-8451).

## What a durable seat is

A durable seat is a long-lived agent that a desk holds, outside any epic. For
example, the PR REVIEW desk holds `LSA • GENERAL` and
`LSA • PR DRAFTS STATUS CHECKER`. Each seat is a dated decision that names
who may restart it:

| `recovery_owner` | Who runs and recovers the seat | What Kontor does |
| --- | --- | --- |
| `paseo_direct` | An operator, directly in Paseo | Records and reports the declaration. Never censuses, adopts, launches or restarts it. |
| `daemon` | Kontor | Plans a SeatBinding when the seat is declared. Launches the seat as a persistent hosted seat in the desk workspace. Restarts it only when an operator selects it and the runtime proves it missing. |

A daemon-owned seat uses the existing hosted-seat machinery unchanged:

- the SeatBinding id is the runtime correlation label;
- a launch intent is prepared before the effect, and the native that comes
  back is bound, installed and observed;
- a restart moves the absent native into the seat's append-only route
  history (which fences its credential) and launches the next occupancy
  generation.

There is no new scheduler and no new runtime protocol. The only runtime
addition is a read-only census of the natives that carry one binding label.

## Surfaces

| Operation | `/v1` | MCP tool | Tier |
| --- | --- | --- | --- |
| Read a desk's seats | `GET /v1/projects/{project_id}/desks/{desk_key}/seats` | `kontor_desk_seats_list` | observer |
| Declare a seat | `POST …/desks/{desk_key}/seats:declare` (with `Idempotency-Key`) | `kontor_desk_seat_declare` | operator |
| Reconcile seats | `POST …/desks/{desk_key}/seats:reconcile` (with `Idempotency-Key`) | `kontor_desk_seats_reconcile` | operator |

The CLI is generated from the same tools. They are served by an MCP server
with no serve profile. No restricted profile (worker, consultation,
leadership, planning-pair member or caller) includes them.

Receipts name the project and use two new kinds, `declare_durable_seat` and
`reconcile_durable_seats` (schema 132). Like every command receipt, a receipt
records that the command was accepted. Read the seats again to see what the
runtime reports.

## Declaring a seat

```json
{
  "expected_revision": 41,
  "seat_key": "general",
  "role_code": "LSA",
  "seat_label": "GENERAL",
  "procedure": ".github/skills/asma-pr-review/SKILL.md",
  "recovery_owner": "paseo_direct",
  "decided_by": "Igor",
  "decided_on": "2026-10-08"
}
```

A daemon-owned seat must also declare its exact route, for example
`"model_route": {"provider": "codex", "model": "gpt-5.6-sol", "effort": "xhigh"}`.

Rules:

- **The desk must be ensured.** Otherwise the answer is `not_found`.
  Declaring does not need the desk to be materialized; launching does.
- **The title is rendered, not chosen.** It is `role_code` and `seat_label`
  joined by the desk's pinned separator, for example `LSA • GENERAL`. The
  role code must be in the bundled role catalog, and the label may not
  contain the separator glyph.
- **One exact route.** A route names a provider and model from the compiled
  catalog. A named chain such as `premium` is refused, because a declaration
  freezes one route.
- **`procedure` is evidence only.** It must be a plain repository-relative
  path (no leading `/`, no `.` or `..` segments). Kontor never reads it; it
  appears in a daemon-owned seat's launch prompt.
- **`decided_on` may not be after the day the declaration is recorded.**
  There is one day of slack for the decider's time zone.
- **Immutable and permanent.** Database triggers refuse UPDATE and DELETE.
  - Re-declaring the identical decision is `unchanged`, under the same key
    or a new one.
  - A different decision under the same `seat_key` is a `conflict`.
  - A second seat with the same title in the desk is a `conflict`.

  To amend a decision, declare a new seat key.

## Reading a desk's seats

The report lists every declared seat with its `status`, `reason`, current
`native_id` and `occupancy_generation`, and `decision: "none"`:

- A Paseo-direct seat is reported from its declaration alone. Its status is
  always `unchecked` / `paseo_direct_owner`, and no runtime is asked.
- A daemon-owned seat is censused on the runtime that holds its occupancy, or
  on the configured runtime if nothing was ever bound.

| Status | Reasons | Restarted? |
| --- | --- | --- |
| `healthy` | `idle` | no |
| `busy` | `working`, `awaiting_permission`, `errored`, `activity_unknown` | no |
| `missing` | `never_launched`, `launch_not_effected`, `native_absent` | **yes**, when selected with `restart_missing: true` (`decision: "launch"`) |
| `unchecked` | `launch_unconfirmed` | recovered, when selected with `restart_missing: true` (`decision: "recover"`): the native the census saw is bound, nothing new is launched |
| `unchecked` | `paseo_direct_owner`, `runtime_unavailable`, `census_failed` | no |
| `duplicate` | `several_live_carriers` | no |
| `archived` | `native_archived` | no |
| `mismatched` | `binding_retired`, `uncorrelated`, `unrecorded_carrier`, `predecessor_live` | no |

Only `missing` is evidence of absence. A runtime that is not configured,
cannot answer the census, or answers incompletely gives `unchecked`; it never
gives `missing`. A census answer that does not correlate gives `mismatched`.
The typed decision is computed by the pure `classify` and `decide` functions
in `kontor-core`, before any effect.

## Reconciling seats

```json
{ "expected_revision": 41, "seat_keys": ["triage"], "restart_missing": true }
```

- `seat_keys` selects the seats. It needs at least one key, each key once, and
  every key must be declared. Seats that are not selected are never touched.
- `restart_missing: false` observes the selected seats and records a receipt.
  It has no effect.
- `restart_missing: true` with any Paseo-direct seat selected is `forbidden`.
  The refusal comes before any runtime is asked, and no receipt is recorded.
- Otherwise, each selected daemon-owned seat is acted on according to the
  status table above. Seats are handled in key order.
- One reconciliation runs at a time in a daemon. A concurrent call waits, then
  observes what the first one bound, so a missing seat is launched once.
- **The replay fence.** Repeating a key returns the recorded receipt
  (`applied: "unchanged"`). It re-reads every seat and always answers
  `decision: "none"`. Anything that happened since is a question for a new
  key.

A launch needs the desk to be materialized. A desk that is only ensured is
refused with `placement_blocked`. The seat is observed first, census
included, so the runtime may already have been asked. The refusal comes
before Kontor creates the seat's SeatBinding or launch intent, and before
any launch. The first launch creates the seat's
SeatBinding in the desk workspace, using the seat key as its role slot. The
native is placed in the desk's own native project, which is read back by
exact id. It carries the seat's correlation labels and no epic or
mini-project label.

## Operator runbook: reconcile a desk's seats

1. **Desk.** Ensure and materialize the desk, then confirm it through
   `topology:inspect`. See the desk reference.
2. **Declare.** Declare each seat once. Declare an existing Paseo-direct
   seat as `paseo_direct`. Declare `daemon` only for a seat Kontor should
   own from now on. Kontor never takes over a running Paseo-direct seat.
3. **Read.** `GET …/seats`. Act only on what the status table says.
4. **Report first.** If you need a recorded observation, reconcile with
   `restart_missing: false` and a new key.
5. **Restart.** Reconcile the selected daemon-owned seats with
   `restart_missing: true` and a new key. Check each seat's `decision`,
   `native_id` and `occupancy_generation` in the answer, then read the seats
   again.
6. **If the call fails or its answer is lost, retry with the same key.** A
   launch whose answer was lost leaves a prepared intent. The retry reports
   `unchecked` / `launch_unconfirmed` and binds the native the census found.
   It never launches a second one. This also holds across a daemon restart.
   If the runtime returns a native the census did not see, the answer is
   `stale_binding` and nothing is bound.
7. **Check later with a new key.** The same key only replays the recorded
   reconciliation (see the replay fence).
8. **Anything not `missing` is manual:**
   - `busy`: wait.
   - `duplicate` or `unrecorded_carrier`: find the extra native in Paseo and
     archive whichever is not Kontor's recorded occupant.
   - `archived`: the seat was stopped on purpose. Kontor will not restart it.
   - `census_failed` or `runtime_unavailable`: fix the runtime, then read
     again.
   - `binding_retired`: the seat's binding is closed for good. Declare a
     new seat key.

## Support limits

- **Paseo-direct seats are never touched.** There is no census, no adoption
  and no restart for them, even when the runtime shows them missing.
- **Operator-selected only.** Nothing reconciles on a schedule, at daemon
  start, or for seats that were not selected.
- **No dedicated lifecycle operations.** There is no desk-seat operation to
  wake, retire or archive a seat; reconciliation only launches and recovers.
  - A launched daemon-owned seat can be messaged directly through the
    generic seat message route,
    `POST /v1/projects/{project_id}/seat-bindings/{seat_binding_id}/messages`
    (`kontor_topology_seat_message_send`, operator).
  - The generic seat retire route (`kontor_seat_retire`) refuses a desk seat
    with `placement_blocked`, because it fences retirement through an epic.
  - A Paseo-direct seat has no SeatBinding, so no Kontor route reaches it.
  - Stop a daemon-owned seat by archiving it in Paseo. It is then reported
    `archived` and never restarted.
- **Not atomic across seats.** If a later seat fails after an earlier seat
  in the same call was launched, the earlier seat stays launched and bound,
  and no receipt is recorded. A retry with the same key finds that seat
  healthy and does not launch it again.
- **Runtime support.** The census needs the runtime's inspect capability.
  The Paseo adapter and the scripted fake implement it. Any other runtime
  reports `runtime_unavailable`, so its seats are never restarted.
- **Backup.** Export generation 15 carries `durable_seat_declarations`. An
  older generation cannot prove that the declarations are complete, so it is
  refused for any schema-132 database, whether or not it holds any. An
  import records declarations as lineage and declares nothing in the
  destination.

## Rollback

- **Declarations stay.** They are permanent. Leaving them in place is the
  supported rollback state, and an unused declaration has no effect.
- **Stopping Kontor's seats.** Archive the native in Paseo. Kontor reports
  it `archived` and never restarts it.
- **Binary rollback.** Schema 132 rebuilds `command_receipts` to admit the
  two new receipt kinds, and a pre-132 build refuses a schema-132 database.
  Restore the snapshot taken before the upgrade, per
  [`RECOVERY.md`](../../RECOVERY.md).
