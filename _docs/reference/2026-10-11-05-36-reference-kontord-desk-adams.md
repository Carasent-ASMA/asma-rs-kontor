# Desk Adams in kontord

> Status: ASMA-8452 (ASMA-8449) draft candidate. It describes what the desk Adam
> contract supports at this revision, what it refuses, and which evidence is
> still open until the rebase onto ASMA-8451. Naming rules are in
> [`docs/NATIVE_NAMING.md`](../../docs/NATIVE_NAMING.md#desk-adams-asma-8452).
> Desks themselves are described in
> [Durable desks in kontord](2026-10-10-23-55-reference-kontord-durable-desks.md).

## What a desk Adam is

A desk Adam is the leadership session of one epic, or of one ad-hoc topic,
placed in the `ADAM` desk's workspace instead of in an ECP inside the epic.
Desk membership and work subject are kept apart:

- Every epic and every ad-hoc topic gets a **subject**: a topology node of its
  own directly below the desk's workspace. The epic subject is scoped to its
  epic; the ad-hoc subject is scoped to none.
- A subject owns no native container. Its kind declares only
  `parent_session_host`, so its sessions run in the workspace's container,
  with the workspace's project, workspace and working directory.
- Seats are bound to the subject, never to the workspace. The epic a seat acts
  for is its subject's epic. Sharing the workspace never gives one subject's
  seat any authority over another's work.
- One `desk_adam_subjects` row (schema 133) per subject is written before its
  node, and it carries the node id, so a retry reconciles the same subject. The
  row pins the Team Definition revision and the frozen topic. It is immutable
  and permanent: database triggers refuse UPDATE and DELETE.
- There is at most one epic subject per epic, and a subject hosts at most one
  live session at a time. The store enforces both.

## Declaring desk leadership

A Team Definition opts in with `desk_leadership`:

```json
"desk_leadership": { "desk_key": "adam", "epic_kind": "ADE", "ad_hoc_kind": "ADH" }
```

- `desk_key` must be a desk this revision declares.
- `epic_kind` and `ad_hoc_kind` must be distinct containers whose parent is
  that desk's workspace kind and whose only projection capability is
  `parent_session_host`.
- Both must declare a seat template, and the ad-hoc template cannot render
  `EPIC_JIRA_KEY`.
- A definition without the field keeps its exact canonical bytes and hash. An
  epic pinned to one keeps the historical ECP.

No bundled revision declares desk leadership. A project publishes a successor
topology and Team Definition through `/v1`, then selects it, exactly as the
desk reference describes. An epic applied while that revision is selected is
desk-led for its whole life, through its own pin.

A desk-led epic persists only its LSA and TPM seats. An apply whose Core Team
roster declares any other persistent seat is refused (`placement_blocked`)
before anything is written.

## Supported operations

| Operation | Surface | Effect |
| --- | --- | --- |
| Place an epic's Adam | `POST /v1/projects/{id}/epics:apply` under a desk-led selection | The epic subject and its node below the desk workspace; no ECP |
| Launch the planning LSA | `POST …/epics/{epic}/core-team/seats:materialize` with one `LSA` route | `LSA • <epic key> • <topic>` in the desk workspace |
| Hand off to the TPM | `POST …/epics/{epic}/core-team/desk-adam:handoff` | Fences the LSA, then launches `TPM • <epic key> • <topic>` |
| Open an ad-hoc Adam | `POST /v1/projects/{id}/desk-adams/ad-hoc:open` with `topic` | The ad-hoc subject, its node and its LSA seat; no native launch (see limits) |
| Bind an ad-hoc origin | `epics:apply` with `desk_adam_origin: {"subject": <node id>}` | Fences the ad-hoc seats, then records the epic subject with that origin |
| Deliver | `topology:materialize`, `execution:arm`, `scheduler:start` as for any epic | ESW and TSW as before; the delivery owner is the subject's `tpm` seat |
| Complete and close | `completion:advance`, epic `lifecycle` `close_epic` | Answers to the subject's TPM; closing releases only that seat |

MCP serves the handoff as `kontor_desk_adam_handoff` and the ad-hoc open as
`kontor_desk_adam_ad_hoc_open`, both at admin tier. `kontor_epic_apply` takes
the optional `desk_adam_origin`. Receipts are `hand_off_desk_adam` (target:
the epic) and `open_ad_hoc_desk_adam` (target: the project). A binding is
part of the `epics:apply` receipt, whose intent names the origin.

### Placement and the one live session

`seats:materialize` on a desk-led epic routes exactly one role per call while
another of the subject's seats holds a live session. Routing the TPM next to a
live LSA is refused (`placement_blocked`) before any native effect. The
handoff is the only way to move the subject from planning to coordinating.

### Handoff

The request names the exact predecessor and the successor:

```json
{
  "expected_revision": 3,
  "from_seat_binding_id": "<the subject's LSA>",
  "expected_native_id": "<its live native id>",
  "expected_generation": 1,
  "to_seat_binding_id": "<the subject's TPM>",
  "model_route": { "provider": "codex", "model": "gpt-5.6-sol", "effort": "xhigh" }
}
```

1. The epic must be desk-led and placed. Both seats must be non-terminal seats
   of this epic's subject, with roles LSA and TPM, and the predecessor must be
   exactly that native at that generation. Anything else is refused before
   any effect: `placement_blocked` for a wrong seat, `revision_conflict` for a
   moved session, `stale_binding` when the desk workspace no longer proves.
2. The predecessor's vendor and the successor route's vendor must both be
   known and must differ (see limits).
3. The LSA is retired natively in the desk workspace and its hosted row is
   archived. Only then is the TPM materialized, through the same path as
   `seats:materialize`, under a key derived from the handoff's.

A replay with the same key returns the original receipt. A retry after a lost
answer resumes from the step it reached: an LSA that is already archived at
the named generation is not fenced again, and a retire whose acknowledgement
was lost is proved absent and archived.

### Ad-hoc Adams and binding an origin

`ad-hoc:open` with a `topic` records the ad-hoc subject on the leadership
desk, its node, and one LSA seat titled `LSA • ADHOC • <topic>`. A replay, or
another open of the same topic, converges on the same subject. A topic that
has already moved to an epic is refused (`placement_blocked`).

An epic adopts an ad-hoc topic declaratively, on `epics:apply`:

```json
"desk_adam_origin": { "subject": "<the ad-hoc subject's node id>" }
```

- Absent, the request bytes, the canonical intent and the behaviour are
  exactly those of a plain apply.
- Every check runs before any graph write. The epic must be desk-led on the
  same desk. The origin must exist, must be an ad-hoc subject on that desk,
  and must be bound to no epic. None of its seats may hold a native session.
  Any other case is refused, and nothing is written.
- The ad-hoc seats are retired before the epic subject row is written, and the
  store refuses the row otherwise. A bound ad-hoc subject never seats or hosts
  anyone again, and nothing on it is rewritten.
- The epic subject records the origin and freezes the epic's own name as its
  topic. The epic's LSA then launches fresh, with a fresh credential, and its
  initial prompt carries a canonical `desk_adam_binding` capsule naming the
  source subject, its topic, when it opened, the fenced seats and the target.
- A replay with the same origin converges. A different origin for an epic
  that is already placed is refused (`placement_blocked`).

## Support limits

- **No same-vendor handoff.** A handoff whose LSA and TPM routes share a
  vendor, or where either vendor is unknown, is refused
  (`unsupported_capability`) and changes nothing. Same-vendor transfer waits
  on Igor's pending decision on seat-credential continuity. Vendors are read
  as the fleet's independence key. Without a fleet, `claude` is Anthropic and
  `codex` is OpenAI.
- **No same-native keep.** `desk_adam_origin.keep_native: true` is refused
  (`unsupported_capability`), pending the same decision.
- **Ad-hoc native launch is open until ASMA-8451.** `ad-hoc:open` with a
  `model_route` is refused (`unsupported_capability`): the ad-hoc LSA needs
  the epic-free hosted-seat launcher that ASMA-8451 lands. For the same reason
  an origin with any natively hosted seat is refused. The ad-hoc native
  evidence, and the fence-then-relaunch tests that need a live ad-hoc native,
  are added when this branch is rebased onto ASMA-8451.
- **Native evidence is the fake runtime's.** Every native effect here
  (placement in the desk workspace, retire, relaunch, restart resolution) is
  proved against the scripted fake. The live Paseo launch moves onto
  ASMA-8451's launcher at the rebase. Desk container preparation stays
  epic-free, so the adapter never records a desk workspace under an epic's
  project.
- **Backlog import.** `desk_adam_origin` is accepted only on `epics:apply`;
  a backlog import that carries it is refused (`invalid_request`).
- **Titles never retitle.** A later rename of the epic leaves every subject
  title as it was frozen.

## Schema, backup and rollback

- Schema 133 adds `desk_adam_subjects` and rebuilds `command_receipts` for the
  two new command kinds. Schema 132 is reserved for ASMA-8451: this branch
  carries a no-op placeholder at 132 that is dropped at the rebase.
- Export generation 15 carries the subject records. ASMA-8451 takes 15, so
  the subjects move to generation 16 at the rebase. Older generations cannot
  represent a database that holds subjects and are refused for one.
- Subjects are permanent. Selecting a definition without `desk_leadership`
  stops *new* epics from being desk-led. Epics already pinned stay desk-led.
- A pre-133 build refuses a schema-133 database. Restore the snapshot taken
  before the upgrade, per [`RECOVERY.md`](../../RECOVERY.md).
