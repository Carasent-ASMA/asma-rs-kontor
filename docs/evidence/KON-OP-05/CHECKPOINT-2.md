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

## Blocker — there is no authenticated requester identity to resolve

Recorded 2026-08-18, after the TPM remediation dispatch restated CP2c item 1 as
"resolve authenticated requester identity to an exact SeatBinding+generation".

That is not implementable as stated, and the reason is in the authentication
model rather than in OP-05:

- `Caller` is `Caller(pub CallerCapability)` — a one-field tuple carrying an
  authority tier and nothing else (`kontor-api/src/lib.rs:78`).
- The realm holds exactly three bearer secrets, one per tier: observer, operator,
  admin (`kontor-api/src/auth.rs:70`). `authenticate` resolves the presented
  bearer to one of those tiers and inserts `Caller(authority)` into the request
  extensions (`kontor-api/src/lib.rs:151-164`). Nothing else about the caller is
  carried.

Every Operator therefore presents the *same* credential. There is no account, no
AgentRun and no principal in the request path, so no server-side lookup can turn
"the caller" into an exact `SeatBinding`: the information does not exist at the
boundary. "+generation" has no `SeatBinding` analogue either — the generation
concept in this codebase is `RuntimeBindingSnapshot::binding_generation`, which
is a native runtime binding, not a seat.

`advisor_runs.requester_seat_binding_id` is `NOT NULL` and the rows are immutable
evidence, so whichever reading is chosen is inherited by every consultation ever
recorded. This seat is not choosing it silently.

### Options

1. **Accept a claimed requester in the request body.** Rejected: the architecture
   requires the server to resolve the requester and forbids caller-authored
   identity, and this would let any Operator credential attribute advice to any
   seat.
2. **Derive it from durable state — the epic's ECP control seat (the LSA).**
   Implementable today, exact, server-side, not caller-authored, and it already
   has precedent: `applications.rs:11769` finds the epic's control node by
   `delivery.control_kind` and takes the non-terminal binding holding
   `control_slot()`. It matches the architecture's phrase "the requester's or
   owning LSA's disposition". What it changes is the *meaning* of requester: the
   consultation is attributed to the epic's owner rather than to whoever called.
3. **Introduce per-principal credentials.** The only option that makes CP2c item 1
   literally true. It is a change to the realm credential model, not to OP-05, and
   is ADR-scale.

### Recommendation

Option 2, with the semantic stated on the record: `requester_seat_binding_id` is
the epic's control seat, and it means "the consultation was requested under the
authority of the epic's owner", not "this human called the route". Option 3 is
the right long-term answer and should be its own ticket.

This is the one thing in the remediation brief this seat cannot proceed on
without ratification, because it is unreversible once evidence rows exist. The
rest of CP2c — profile loading, scope resolution, limit enforcement, context
freezing, preallocation, exact-ID placement, settlement, scope resolution — is
unblocked and ready to compose behind that one decision.
