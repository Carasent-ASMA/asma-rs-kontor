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

## CP2c — ratified semantic recorded, and two consequences found

The architect ratified (2026-08-18) that `requester_seat_binding_id` means: *the
exact ECP LSA `SeatBinding` under whose durable epic-owner authority the
consultation was requested*. It does **not** identify the operator bearer, human
or runtime session that submitted the request, and OP-05's audit records must not
be read as implying per-principal identity — that remains a separate
credential-model concern.

Composing the resolution against that semantic surfaced two things that change
what CP2c can implement. Both are verified, not inferred.

### 1. The ECP control slot is TPM, not the LSA

`OperationalDelivery::control_role_code` is `TPM`
(`kontor-profiles/fixtures/operational-domain.json`), and `control_slot()`
derives its slot from that code. Resolving owner authority through
`control_slot()` would therefore have bound every consultation to the epic's
*programme* seat while claiming it was the owner's.

The LSA is identified by its standard role code, which is how the promotion
handoff already finds it: `applications.rs:7430` filters the materialized roster
for `role.role_code == MANDATORY_LEAD_ROLE` (`"LSA"`). The resolution follows
that precedent, filtered to non-terminal bindings on the epic's ECP node, and
requires exactly one — refusing `placement_blocked` when the authority is absent,
released or ambiguous, before the run row and before any external effect.

### 2. Constraint 4, enforced literally, makes every consultation unreachable

The ratified constraints require `allowed_caller_roles` to be enforced against
the resolved owner-authority role. That role is always the LSA. The bridge from a
profile's `RoleKey` to a standard role code is
`OperationalDelivery::role_code`, and the shipped bindings are:

| bound code | AUD | BA | QA | SA | SWE | TW | UAT |
| --- | --- | --- | --- | --- | --- | --- | --- |

`LSA` is **not among them**. It exists in the role catalog but no logical role is
bound to it. So no `allowed_caller_roles` entry can ever resolve to the owner
authority's role code, and a literal enforcement refuses every invocation —
including the shipped `independent_review@1` and the Advisor preset shape, whose
`allowed_caller_roles` is `["architect"]` → `SA`.

This is a consequence of the ratified derivation rather than a defect in it: once
the requester is *always* the epic owner, a list of permitted caller roles has
nothing variable left to constrain.

#### Options

1. **The owner authority is always permitted; `allowed_caller_roles` is recorded,
   not enforced, until a requesting principal exists.** Coherent with the ratified
   semantic — the epic's owner cannot be unauthorized to consult about its own
   epic — and it keeps the declared list meaningful for the day per-principal
   identity arrives. It does mean constraint 4 is satisfied vacuously today, which
   must be stated rather than quietly true.
2. **Bind a logical lead role to `LSA` in the operational-domain data** and require
   profiles to declare it. Makes constraint 4 literally enforceable, costs one
   data entry, but adds vocabulary whose only purpose is to satisfy a check whose
   subject is already fixed.
3. **Enforce constraint 4 as written and ship it unreachable.** Rejected: it would
   pass its own tests and admit no consultation.

#### Recommendation

Option 1, with the vacuity recorded here and in the run projection, and option 2
revisited when per-principal identity lands. This seat is not choosing between
them silently, because constraint 4 was given as mandatory and either reading
changes what an OP-05 audit record asserts about authority.

### State

No CP2c code is committed. The resolution, scope, profile-loading and
context-freezing helpers were composed and compile, but `clippy -D warnings`
correctly refuses them as dead code without the caller that constraint 4 blocks,
so they were reverted rather than landed as scaffolding. The tree is at
`e7f0148` behaviour: all five run operations refuse, `resolve_scope` rejects both
consultation targets.

## CP2c — the ratified requester semantic and the role-type ruling

Two architect rulings govern CP2c. Both are recorded here because both change
what an OP-05 audit record asserts.

### Ruling 1 — what `requester_seat_binding_id` means (2026-08-18)

*The exact ECP LSA `SeatBinding` under whose durable epic-owner authority the
consultation was requested.* It does **not** identify the operator bearer, the
human or the runtime session that submitted the request: the realm holds one
bearer secret per authority tier, so no principal exists at the boundary to
identify. OP-05's records must not be read as implying per-principal identity,
which remains a separate credential-model concern.

Resolution is server-side from the epic and its ECP, never from the request body,
and fails closed before the run row and before any external effect when the
authority is absent, released or ambiguous. The resolved binding is preserved
immutably on each run, so a later LSA replacement affects only future runs.

One correction found while composing it: the LSA is **not** reachable through
`control_slot()`. That derives from `OperationalDelivery::control_role_code`,
which is `TPM` — the epic's programme seat. The LSA is identified by its standard
role code, exactly as the promotion handoff already does at
`applications.rs:7430` (`role.role_code == MANDATORY_LEAD_ROLE`).

### Ruling 2 — caller roles are catalog codes, not logical keys (2026-08-18)

Enforcing a caller allowlist against the resolved owner authority was
unsatisfiable while `allowed_caller_roles` held Foundation logical `RoleKey`s:
the bridge to a standard code is `delivery.role_bindings`, whose bound codes are
`AUD`, `BA`, `QA`, `SA`, `SWE`, `TW` and `UAT`. `LSA` exists in the role catalog
but is bound to no logical role, so no allowlist entry could ever match the epic
owner and every invocation would have refused.

The architect ruled the type boundary rather than the data:

- `allowed_caller_roles` is `Vec<RoleCode>` — standard catalog roles, validated
  against the catalog itself.
- `independent_review@1` now admits `["LSA", "SA"]`, which keeps non-lead
  architect eligibility while explicitly admitting the current owner authority.
- `CommitteeSlotSpec.logical_role` stays a `RoleKey`. Only consultation *members*
  resolve through `delivery.role_bindings`.
- `LSA` is deliberately **not** added to `delivery.role_bindings`, which would
  make it selectable as an Advisor or Committee member role — outside the
  intended boundary.

The daemon's preview guard now checks the two along their own boundaries: caller
codes against the role catalog, member logical roles against the delivery
bindings. The resolved owner seat's catalog role is compared directly with the
allowlist before a run row or any native effect exists.
