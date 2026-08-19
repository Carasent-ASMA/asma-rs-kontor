# KON-OP-08 checkpoint 1 — staged multi-hop ticket convergence

Date: 2026-08-19
Status: implemented; scoped gates green; **not pushed** (see schema lineage below)
Against: `docs/evidence/KON-OP-08/2026-08-19-11-39-architecture-kontor-operational-control-surfaces.md`

## The live evidence this answers

A reconcile plan for OP-08/OP-12 read `DRAFT` and produced `implementation_active`
targeting `In Development`. Apply returned 200 but performed only the assignment
prerequisite. A fresh readback still saw `DRAFT`, assignee Igor, and then
conflicted — because this Jira workflow offers **no direct `DRAFT -> In
Development` transition**. From `DRAFT` it offers `READY FOR DEVELOPMENT`
(`10213`) via transition `15`, and nothing that reaches `In Development`
(`10214`).

The shipped specification corroborates every id:

| Fact | Value | Source |
| --- | --- | --- |
| `DRAFT`, declared *and* inbound-compatible | `10237` | `fixtures/external-workflow-asma.json` |
| `reopen` selector | `10213` READY FOR DEVELOPMENT | same |
| `implementation_active` target | `10214` In Development | same |

So the failure is exactly `NoLiveTransition`, reached after the assignment
prerequisite converged. Both prior behaviors were wrong to ship: forcing the move
would invent a route the workflow refuses, and reporting convergence would call an
unconverged ticket done.

## What changed

`kontor_core::ticket::reconcile` gained one narrow allowance. When the milestone
target is not reachable in one move, it may route through **the status the pinned
specification already declares as its reopen selector**, and only when that status
is directly reachable right now.

It is deliberately **not** a path search. A shortest-path walk over whatever
transitions happen to be live would let the evaluator route a ticket through a
status the workflow owner never approved. `staged_hop` is fail-closed on every
other shape:

| Shape | Outcome |
| --- | --- |
| declared intermediate, directly reachable, exactly one route | staged hop |
| no `reopen` selector declared | `NoLiveTransition` |
| intermediate declared but not currently offered | `NoLiveTransition` |
| ticket already standing on the intermediate | `NoLiveTransition` (a hop there makes no progress, and re-planning it after the next observation is how a hop becomes a loop) |
| intermediate is itself the target | `NoLiveTransition` |
| intermediate not a status the specification classifies | `NoLiveTransition` |
| several live routes reach the intermediate | `MultipleLiveTransitions` |

The plan keeps naming the milestone it is converging to; only its *attempt*
changes. Two accessors state that distinction:

- `TransitionPlan::destination()` — where **this attempt** lands: the transition's
  own destination when there is one, else the target.
- `TransitionPlan::is_staged_hop()` — the attempt stops short of the milestone on
  purpose; the milestone is reached by the next observation.

Two applier sites had assumed `transition.to == target` and were corrected to ask
`destination()`:

- `reconcile_after_ambiguity` — would otherwise have judged a hop that landed
  exactly as planned `Contradictory` and invited a retry of a move Jira had
  already made.
- `prove_live_route` — the fail-closed guard that the route is still offered; it
  now proves the route reaches *this attempt's* destination.

## Why no new conflict kind and no new plan column

Both were the obvious designs and both are refused by a constraint outside this
change:

- `status_conflicts.kind` carries a SQL `CHECK (kind IN (...))` enumerating the
  ten conflict kinds (`0001_init.sql`). A new `StatusConflictKind` variant needs a
  migration.
- `assignment_prerequisite` is a persisted column, so a sibling "this is a hop"
  flag needs one too.

The schema lineage is frozen (below), so the design uses what is **already
representable**: a hop is exactly `transition.to != target`, which the persisted
plan already records. No migration, and a receipt still shows precisely which hop
it made.

## Gates

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | pass |
| `clippy` on core, integrations-asma, store, daemon, `--all-targets -D warnings` | pass, no warnings |
| `cargo test -p kontor-core -p kontor-integrations-asma --all-targets` | pass |
| `cargo test -p kontor-store --all-targets` | pass |
| `cargo test -p kontor-daemon --all-targets` | pass |

New tests:

- `a_target_two_moves_away_is_reached_through_the_declared_intermediate` — the hop
  is planned, still names the milestone, uses the offered route id, and the second
  observation converges directly.
- `an_intermediate_kontor_was_not_given_is_refused_rather_than_invented` — the
  whole fail-closed matrix above.
- `a_draft_ticket_reaches_in_development_through_ready_for_development` — the live
  case, against the **shipped** specification, asserting `10237`/`10213`/`10214`
  read from that specification rather than retyped.

**Mutation proof.** Replacing the `staged_hop` arm with the original
`return Conflict(NoLiveTransition)` turns both core tests red — including the
fail-closed one, because the ambiguous-hop case then reports `NoLiveTransition`
instead of `MultipleLiveTransitions`. Restored, both pass.

## Not done in this checkpoint

**Typed conflict at the API surface.** The evaluator's conflict has always been
typed; what the live run saw was a `revision_conflict` plus opaque policy prose at
the `/v1` boundary. Surfacing the typed kind — and the routes the observation
actually offered — is a daemon-surface change and is the next slice.

## Schema lineage — do not push

`0041_project_subject_authority.sql`, committed in checkpoint 0 (`785e458`),
collides with OP-12's `0041_open_questions`. **OP-12 merges first.** This branch
must not be pushed or merged as-is.

After OP-12 merges: integrate master, renumber the authority migration to the next
free number, move its terminal `PRAGMA user_version`, update `SCHEMA_VERSION`, and
update the tests that name it — `migration_0041_*` in
`crates/kontor-store/tests/schema_v1.rs`, the v41 comment in `migrations.rs`, and
`the_committed_inventory_is_the_surface_this_workspace_serves` if the surface
moved. Then rerun the full gates.

The checkpoint-0 convergence fix already makes the hardcoded lineage branch
self-extending, so that renumber will not silently strand `user_version` again.
