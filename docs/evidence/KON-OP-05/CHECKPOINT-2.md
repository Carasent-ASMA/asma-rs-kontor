# KON-OP-05 Checkpoint 2 — progress

Date: 2026-08-18
Author: builder seat
Status: **in progress** — two of four units landed
Base: `c614fab` (Checkpoint 1 with its review findings closed)

The architect holds the release verdict. This document records what is composed
and what is not; it makes no claim about the gate.

## Landed units

| Unit | Submodule | Superproject | Gate |
| --- | --- | --- | --- |
| CP2a — consultation persistence | `e1ea968` | `420f1351` | 1454 tests, clippy, fmt |
| CP2b — run wire contract | `e7f0148` | `197014f3` | 1454 tests, clippy, fmt |

### CP2a — durable consultations

Migration `0034` adds `advisor_runs`, `advisor_advice` and
`advisor_dispositions`. The run row freezes everything a consultation was asked
under before any native effect: profile revision by id/version/canonical hash,
question bytes and digest, resolved context document and provenance, and the
exact ASW node, seat, slot, catalog role and epic ESW that will answer.

`UNIQUE (project_id, intent_hash)` is the lost-acknowledgement guard, following
`quick_sessions`: the retry loses the race, has written nothing, and reads the
winner rather than placing a second ASW and spending a second consultation
against the profile's declared limit.

Advice is one row per run behind an immutability trigger — submitting it is the
whole of an Advisor's write authority. Dispositions are append-only and keyed by
sequence, so `superseded` adds a decision beside the earlier one instead of
erasing that the advice was once rejected. `AdvisorRunState` has no `running`.

Eight store tests: frozen round-trip, one-invocation-one-consultation, retry
reconciliation by intent, compare-and-swap advance, advice written once,
dispositions appending rather than replacing, per-epic listing, project scoping.

This is the ninth rebuild of `command_receipts` and the first to re-create its
own triggers, which the v33 restoration and its schema pin now require.

### CP2b — the wire contract

`InvokeConsultationRequest` gains a closed `scope` (`epic`, or a `ticket` of that
epic) with no native placement, parent or kind in it. The shared bodyless
settlement request is replaced for the Advisor family by a tagged action, because
two authorities were hiding behind one body: `record_advice` is the Advisor's own
bounded output, `record_disposition` is the requester's or owning LSA's decision
about it, and `needs_human` carries a recommendation and what was tried.
`AdvisorRunDto` now projects resolved scope, ASW node, seat, question and
context digests, the advice digest once durable, the ordered dispositions and the
run revision.

`contract/openapi.json` regenerated; MCP tool specs extended. The parity suite
caught that Committee invoke shares the request DTO and needed the same argument.

## Not composed

All five consultation run operations still return typed `Unavailable`, and
`Services::resolve_scope` still refuses `AdvisorConsultation` and
`CommitteeConsultation`. Nothing yet places an ASW, freezes a context, records
advice or a disposition, or reaches `needs_human`.

Remaining CP2 work, in the order it has to happen:

1. **Invoke.** Resolve the pinned profile revision and parse it back from its
   canonical bytes; resolve the closed scope, refusing a ticket that belongs to
   another epic; hold the profile's `allowed_scopes` and consultation limit;
   freeze the context; write the run row; then reconcile the ASW node, the one
   Advisor seat and the native container by the ids that row already froze.
2. **Settle.** The three actions against `AdvisorRunState`, each under
   compare-and-swap, with advice refused after `advised` and a disposition
   refused before it.
3. **`resolve_scope`.** Accept both consultation targets by reading the durable
   run's epic, which is now storable but not yet written by anything.

CP3 (Committee runs) and CP4 (round lineage, `NEEDS_HUMAN` completion, stub
replacement) are untouched.

## Validated design notes for the remaining work

Recorded because they cost real reading and the next seat should not repeat it.

- **Context freezing has a usable entry point.** `kontor_context::preview` takes a
  `ResolutionRequest { realm_id, sources, references }` and returns a
  `ResolvedContextPack` carrying the canonical document, provenance and
  redactions. `ResolvedContextPack::new` is crate-private, so `preview` is the
  only way in — build `ContextSource` values per layer (`ProjectProfile` for the
  profile policy, `Scope` for the epic/ticket identity, `TeamRoleProfile` for the
  Advisor role, `TaskAdditions` for the question) rather than constructing a pack.
- **Placement has a template.** `ensure_quick_session` is the closest existing
  shape: one node, one seat, row-first with pre-frozen ids, each effect
  reconciled by lookup before creation, `placement_blocked` before any effect when
  the pinned specification does not declare the kind a session host. The ASW path
  differs only in having an epic ESW parent instead of the project session base.
- **Command targets.** `InvokeAdvisorRun` and `SettleAdvisorRun` witness the epic.
  A consultation is not a Task and not a TeamRun, so neither is a legal target,
  and `AggregateRef` needed no new variant.

## Open question for the architect

OP-03 requires "Operator plus server-side SeatBinding authority" for the run
operations, and the run row has a mandatory `requester_seat_binding_id`. I could
not find an existing daemon helper that resolves an authenticated caller to a
`SeatBinding`; the composed operations that need a seat either take a role code
and resolve a catalog role, or read the seat from a node they already hold.

Two candidate readings, and this seat should not pick one silently:

1. the requester seat is the epic's control seat (the ECP's LSA), so a
   consultation is requested on behalf of the epic's owner; or
2. the caller's credential resolves to a seat directly, which needs a mapping
   that does not appear to exist yet.

Reading 1 is implementable today against OP-04's ECP and matches the
architecture's "the requester's or owning LSA's disposition". It is what the next
unit will use unless the architect says otherwise, and it is recorded here rather
than buried in code.
