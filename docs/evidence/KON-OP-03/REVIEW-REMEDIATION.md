# KON-OP-03 — remediation of the rejected code-review gate

Answers rejection receipt `01a00d02-7675-7331-8802-c3d3f973c16d` (gate
`code-review-gate`, sequence 1) and the notes in `REVIEW.md`.

## M1 — formatting

`cargo fmt --all --check` was red on five lines the reviewed commit added.
Formatted and committed on its own so the fix is separable from the behaviour
changes.

## M2 — the shared vocabulary had no consumers

Seven DTOs shipped in `contract/openapi.json` and the console's `schema.d.ts`
with no operation able to return them. They are wired rather than deleted: the
Delivery Team slot consumes `RoleSelectionDto`, and the semantic-topology family
consumes the other six.

Reachability is now checked transitively from the operations, not by eye. Every
schema the contract declares is reachable except seven that were already
unreachable before this ticket: `ApiErrorBody`, `EventDto`, `ReceiptDto`,
`RunDto`, `StreamFrameDto`, `StreamRefusalDto`, `TaskDto`. Removing
`/v1/commands/{kind}` orphaned one more — `ReceiptResponse`, its response type —
and that one is deleted, because it was this ticket that stranded it.

## M3 — the Delivery Team slot

`TeamDraftRequest.slots` was `Vec<serde_json::Value>`. A slot now carries a
typed `RoleSelectionDto`: a catalog revision, a role code, an optional display
label. The console follows, in its seeds and in the slot a new template starts
with.

The required negative proof is killed by the type rather than by a check. There
is no field for a standard title, and the field list is closed — without
`deny_unknown_fields` serde discards an unknown field and answers 200, which a
caller reads as agreement. The proof pins three shortcuts: a bare role string, an
unparseable code, and a smuggled `standard_title`. Removing
`deny_unknown_fields` makes the third assertion fail with 200 instead of 422,
which is how we know it is testing what it claims.

The draft response echoes the selection rather than a resolved role. There is no
catalog service yet, and a standard title invented here would be the second
source of truth the selection type exists to remove.

## M4 — the generic command route

`/v1/commands/{kind}` is gone from the router, the generated contract and
`NON_AGENT_ROUTES` (now two entries: health and the contract document). Its
handler, its `command_authority` map and the two helpers only it used went with
it, along with a stale contract assertion claiming the console called it — the
console never did.

The proofs it carried were ported first, onto concrete operations:

| Proof | Now driven through |
| --- | --- |
| a receipt names its realm | `POST /v1/teams/drafts:save` |
| an observer may not write | `POST /v1/teams/drafts:save` |
| an operator may not do an admin act | `POST /v1/projects:ensure` |
| a replay answers from what is durable | `POST /v1/teams/drafts:save` |
| a reused key with different bytes conflicts | `POST /v1/teams/drafts:save` |
| a mutation without a key is refused | `POST /v1/teams/drafts:save` |
| a stale revision reports the current one and writes nothing | `POST …/tasks/{task_id}/profile-selection` |
| an unknown target is not found | `POST …/tasks/{task_id}/profile-selection` |
| another realm's ids resolve to nothing | `POST …/tasks/{task_id}/profile-selection` |

One property did not survive, and could not: a command kind targeting an
aggregate it may not target was a fact about the dynamic surface. What replaces
it is structural — a concrete route cannot address the wrong aggregate, because
the aggregate is in its path — plus a test that `POST /v1/commands/{kind}` is no
longer routable at all, so the surface cannot return unnoticed.

Porting the replay proof surfaced a defect worth recording. The Teams writes let
the store's generic conflict become `revision_conflict`, which tells a client to
re-read and retry; a retry never clears a reused key. They now answer
`idempotency_conflict`, as `projects:ensure` already did.

## Surface

| | operations |
| --- | --- |
| before this ticket | 66 |
| after the reviewed commit | 73 |
| now | 116 |

Added: topology specification, catalog and code help (7); semantic topology (8);
native capacity and exact-seat (9); the successor-ticket contracts (27).
Removed: the generic command route (1).

That is checkpoint 1 complete — the whole route table, the shared DTOs, the
authority/idempotency/preview rules, the registry, the generated artifacts, and
the successor routes failing closed — and checkpoint 2's contract half.

Behaviour still lands with the services that own it. Every operation this ticket
adds refuses with a typed `unavailable` before any effect rather than answering
an empty projection, which a caller cannot tell from a project that genuinely has
none.

## Reused shapes

Advisor profiles, Committee templates and Completion profiles are three
aggregates with one wire shape — an identity, a monotonic version, a label and
the digest frozen at publish. They share `ProfileRevisionDto`,
`ProfileCatalogDto`, `ProfilePreviewRequest`, `ProfilePreviewDto`,
`ProfileApplyRequest` and `AppliedProfileDto` rather than carrying three
identical copies that would drift apart the first time one gained a field.

## Two forced deviations from the handoff

**Path spelling.** `ARCHITECTURE.md` writes `{topology_node_id}:retire`,
`{topology_node_id}:archive`, `{advisor_run_id}:settle` and
`{committee_run_id}:settle`. Axum allows one parameter per path segment and
refuses to build a router containing them. The verb moves into its own segment —
`…/nodes/{topology_node_id}/retire`, `…/advisor-runs/{advisor_run_id}/settle`,
`…/committee-runs/{committee_run_id}/settle` — matching `/teams/{team_id}/publish`
and the memory routes. A `verb:noun` suffix on a *literal* segment
(`core-team:apply`, `completion:advance`) routes fine and is kept exactly as the
handoff spells it; only a parameter segment cannot carry one.

**A narrowed mutant guard.** `mcp_mutants.rs` forbids a tool name containing
`archive`, on the reasoning that such a tool would be driving a runtime.
`Archived` is a lifecycle state a topology node already has in the store, so
archiving one is a write about Kontor's own record. The guard now makes one
exception, by exact tool name, so a second archiving tool or a runtime-archiving
one still fails.

## Still open

Behaviour, not contract. Every operation this ticket adds refuses with a typed
`unavailable`; what remains is composing the services behind them.

- **CP2 behaviour** — the topology specification store, the role catalog and
  code help, and the semantic topology operations against OP-01's store and
  OP-02's capability-dispatched materializer.
- **CP3 behaviour** — the account-owned collectors and their typed wire parsing,
  removing the `AsmaExecutable` fleet edge so Kontor passes with `asma` absent,
  and persisting raw observations before deriving availability.
- **CP4 entirely** — the persisted adaptive controller (seeded when an epic is
  pinned, restored on every snapshot, widened only on a second distinct clean
  observation), active-TeamRun accounting against the mission ceiling of seven,
  and the replay/restart, registry-drift and `asma`-absent suites.
- The successor application services themselves are OP-04, OP-05 and OP-06's.

## Pre-existing, unchanged

Authority is checked inside the handler body, so axum's `Json` extractor runs
first and an under-authorized caller sending a malformed body gets 422 rather
than 403. Uniform across the pre-existing routes; the new ones follow the same
shape. `REVIEW.md` reached the same conclusion.

## Gates

`cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`
and `cargo fmt --all --check` are green, as are the console's typecheck, its 278
tests and `verify:api` (the committed `schema.d.ts` is what this crate serves).
