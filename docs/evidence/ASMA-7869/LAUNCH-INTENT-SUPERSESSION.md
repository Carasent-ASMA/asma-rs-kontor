# ASMA-7869 — superseding a never-bound prepared launch intent

Date: 2026-09-19
Carried on the ASMA-8187 candidate, schema line 0100–0106 kept coherent.

## The wedge

Live readback for SeatBinding `01a02b8e-8f63-7161-b043-cf8cc6d1297e` (project
`01a0064a-…`, ECP node `01a00c26-…`, epic `01a0074f-…` / Jira ASMA-7869),
verified read-only before any change:

- binding active, revision 2, role TPM, never released, never replaced;
- `hosted_topology_seat_launch_intents`: occupancy generation 1, state
  `prepared`, autonomy `bounded`, route
  `{"provider":"opencode","model":"deepseek/deepseek-flash","effort":"max"}`,
  `prepared_at` 2026-09-18T20:50:33.373705Z, `installed_at` NULL,
  `observed_native_id` NULL;
- `hosted_topology_seats`: no row. `hosted_topology_seat_history`: no row.

One correction to the brief, from the same readback: three command receipts
mention the binding, not one. Two are `invoke_committee_run` where this seat is
the **caller** (`caller_seat_binding_id`), and one is the read-only
`observe_seat`. None is a launch or effect receipt, so the conclusion stands —
but the fence is written against the seat-binding *field* rather than the whole
document, precisely so a consultation this seat asked cannot be mistaken for an
effect on it.

`prepare_hosted_seat_launch_intent` refuses a second route for one occupancy
generation. That is right while a launch is in flight and wrong forever
afterwards: an intent prepared before an effect that never landed pins the seat
to a route nothing can start, and every later materialization of generation 1
refuses against it.

## What was added

`POST /v1/projects/{project}/epics/{epic}/core-team/launch-intents:supersede`,
admin-only, idempotency-keyed. It replaces **only** the route and prepared
instant of an inert intent. The intent keeps its identity, generation and
`prepared` state, so the launch that follows is the *first* launch of that
occupancy rather than a second one. No binding is created, retired or replaced;
nothing is launched.

Every fence is re-proved inside the one transaction that performs the swap,
because each is a race:

| Fence | Refuses |
| --- | --- |
| binding | missing, not active, released, replaced, or moved revision |
| intent | missing, installed, observed, wrong generation, wrong route, wrong prepared instant |
| absence | any `hosted_topology_seats` row, any `hosted_topology_seat_history` row |
| receipt | any `materialize_core_team`, `correct_core_team_route`, `claim_core_team_seat`, `replace_seat`, `retire_seat` or `launch_run` receipt naming the seat |
| destination | OpenCode, a route identical to the superseded one, a provider the runtime does not offer, or one no governed account may select |
| exactly-once | unique idempotency key; a replay answers, a different intent under the same key conflicts, a second supersession of the same occupancy refuses |

## Schema

Kept on **0106**, extending the unmerged migration rather than adding 0107. It
adds `hosted_seat_launch_intent_supersessions` (STRICT, undeletable, frozen on
commit except the receipt binding) and narrows the v103 immutability trigger
with one carve-out: the route and prepared instant may move only while the row
is `prepared` with no installed instant and no observed native, autonomy,
generation, project and seat unchanged, **and** a matching supersession row
already records exactly what is being replaced. The evidence is therefore
written before the swap, not after.

A live schema-105 realm upgrades by exactly one migration. Proved on a
`.backup` copy of the live realm: 105 → 106, `integrity_check` ok,
`foreign_key_check` clean, both tables present and STRICT, live file untouched
at 105.

## Tests

`a_never_bound_prepared_launch_intent_is_superseded_in_place` ·
`a_launch_intent_supersession_refuses_every_shape_that_is_not_inert` (9 staged
cases) · `a_launch_intent_supersession_refuses_a_seat_with_an_effect_receipt` ·
`a_launch_intent_supersession_is_exactly_once_under_replay_and_drift`.

Identity preservation is asserted in the eligible case: same binding id and
revision, still non-terminal, `released_at` and `replaced_by` still absent, and
still no occupancy.

Two shapes are **not** independently staged, with reasons rather than silence:

- *observed native without installed state* — the v103 schema ties
  `state = 'installed'` to `observed_native_id IS NOT NULL` with a CHECK, so the
  pair moves together and `installed-occupancy` covers both;
- *true concurrent CAS* — the suite stages drift between the caller's read and
  the swap, which is what the store's predicate is written against. A genuinely
  simultaneous writer is not reachable from this harness.

## Mutation evidence

Each seeded alone, observed, then restored; the working tree was compared
byte-for-byte with the pristine copies afterwards and no `MUTANT` marker
remains.

| Mutant | Result |
| --- | --- |
| absence fence removed | **killed** — `…refuses_every_shape_that_is_not_inert` |
| binding-revision CAS removed | **killed** — same test |
| route/prepared-instant predicate removed from the swap | **survived** |
| route predicate *and* trigger evidence clause both removed | **killed** — same test |
| effect-receipt fence removed | **killed** — `…refuses_a_seat_with_an_effect_receipt` |
| OpenCode destination ban removed | **killed** — same test, after the fixture was made able to select OpenCode and the case was made to assert the exact refusal rule |
| exactly-once replay guard removed | **killed** — `…is_exactly_once_under_replay_and_drift` |

The surviving single-removal is reported rather than hidden: the route fence is
genuinely double-guarded, by the store predicate and by the v106 trigger's
evidence clause, and no single removal admits a wrong route. The OpenCode ban
needed two fixture corrections before it was killable — without them the case
refused for unrelated reasons (no account could select OpenCode; the model was
denied at the domain boundary), which would have made the mutation evidence
worthless.

## Not done

Production execution. This is the capability; applying it to the live seat is a
separate operator action under a fresh key.
