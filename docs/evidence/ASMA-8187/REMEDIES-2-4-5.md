# ASMA-8187 — objective remedies 2, 4 and 5

Date: 2026-09-28
Artifact: remediation evidence (additive; every earlier report unchanged)
Task: Jira `ASMA-8187` / Kontor `01a0a943-652b-79d0-9f0c-f4965a33d7ab`
Branch: `feat/ASMA-8187-current-core-team-succession`
Audit pin this work starts from: `408e789590795ad0cab2e43cd1ebb1dbbeebb337`
Preserved old lineage: `6bef9d64dad522a18ca29c82e2fdee50ed5485e3`

The immutable HIGH-VERIFICATION `FAIL` on the old branch stands unchanged. This
document records what was done after it, not a revision of it.

## Remedy 2 — the effect-commit boundary proves its own effects

The latch took two booleans and believed them, so the persistence boundary
recorded the caller's opinion rather than durable state.
`commit_core_team_route_succession_effects` now derives both latches from the
rows the effects were supposed to write: the launch intent for this occupancy
must be `installed` and name this succession's exact successor, and the seat's
active occupant must be that successor with the SeatBinding's attachment
instant equal to the successor's own observation instant. Anything short of both
refuses and latches nothing.

A receipt additionally re-proves its readback where it binds: the digest is
recomputed from the stored bytes, and every value the readback repeats from a
ledger column is compared against that column.

## Remedy 4 — the intervals a succession can actually be lost in

Exclusivity and recovery had been argued from sequential probes. They are now
argued from a deterministic barrier and from seams entered honestly.

**Barrier.** `pause_next_hosted_retirement` holds one apply inside its own
retirement, having taken its claim and launched nothing, while a second arrives.
Two cases: a fresh second key, which must be refused before any native effect;
and the same key, which the claim cannot refuse because the claim is re-entrant
by design.

**Seams, one per real boundary.**

| Boundary | Seam | Recovery under test |
|---|---|---|
| after the retirement | `lose_next_hosted_retire_ack` | the claim owner finishes; one retirement, one successor |
| after the launch | `lose_next_hosted_launch_ack` | the replay adopts the orphan native rather than minting beside it |
| after the route/ledger commit | `lose_next_succession_route_ack` (new) | reconciliation performs *both* effects, neither having been attempted |
| after the receipt, before the binding | `lose_next_succession_receipt_binding` (new) | the recorded receipt is rebound, not duplicated |

A refused launch is arranged separately (`refuse_next_hosted_launch`, new),
because nothing was created and so a replay must still create one — with the
retirement before it already irreversible.

The pre-existing `lose_next_succession_effects` seam fires *after* both effects
have landed, so it proves latch loss only. The route-ack seam is the earlier and
strictly weaker starting state, and the two are now distinguished by reading the
launch intent's own state rather than the latch.

### Two defects the barrier found

1. **The successor occupancy was re-derived after the claim had fixed it.** A
   caller that lost a concurrent race read the seat again, got the winner's
   generation, and then prepared a launch intent, launched, and reported an
   occupancy and grant generation the ledger never recorded. The claim now pins
   it, and the pinned value is read back from the row rather than carried
   forward.
2. **A caller whose transition had already been made answered from its own
   bytes.** `replace_hosted_topology_seat_route` answers `Unchanged` when the
   seat already holds the successor; the caller went on as though its own
   transition had committed and reported a readback — including its own
   `retired_at` — that no ledger row contains. A transition already made now
   makes its caller a converger: it reloads the committed row and answers from
   it, or refuses if there is none.

### Two registries a migration has to move, missed at the port

`crates/kontor-store/tests/publication_attestations_immutable.rs` asserts the
exact schema version, and `repository_roundtrip.rs`'s v115 rewind drops every
artefact introduced after v114 before re-applying. Neither was updated for
migration 0120, so both failed from the port commit onward. Both are fixed, and
with 0121 the full set is now six places: the `.sql` file, the `migrations.rs`
registry and `SCHEMA_VERSION`, `schema_v1.rs`'s table registry and version
assertion, `backup/export.rs`, the publication-attestation version assertion,
and the v115 rewind.

## Remedy 5 — a succession crossing a Realm boundary

### The binding disposition

> The complete succession source row, receipt and readback included, must
> survive import as inspectable, non-live evidence. It must not restore live
> succession, idempotency, credential, grant, placement or materialization
> authority in the destination.

Before this, an import kept only a lineage row — kind, source identity, digest.
A digest over bytes an investigator cannot read proves only that somebody had
them.

### Migration 0121 — `imported_record_evidence`

Content-bearing lineage, and only that. The schema is what keeps preservation
and authority apart:

* the composite foreign key ties each row to an `imported_records` row, which
  ties it to the import receipt that names the source Realm;
* a trigger admits content only beside a `recorded` disposition — not
  `materialized`, not `already_present`, not `refused`;
* update and delete triggers make it unrewritable and undeletable;
* it is in `backup/export.rs`'s excluded-table registry, so imported testimony
  is never forwarded onward as though this Realm were its source.

`PRESERVED_EVIDENCE_KINDS` in `backup/import.rs` is a closed, named list holding
one kind. Membership grants nothing.

A live imported succession is additionally impossible by construction: every
foreign key on `core_team_route_successions` — project, mini project, seat
binding, command receipt — names rows that do not exist in the destination.

### Export generations

A v12 document may neither carry succession rows it has no fields for, nor be
offered by a database new enough to hold them; and the continuity summary must
disclose every one it carries. All three are exercised against a non-empty
v13 document round-tripped through serialize/parse.

### Generation-scoped bearers

Across a real succession on a hosted TPM seat, over the seat-authored
open-question route, which compares the bearer's occupancy generation against
the seat's live one:

* the predecessor's generation-1 bearer still verifies — it is a genuine
  signature over a genuine seat — and is refused on the occupancy it names;
* the successor's generation-2 bearer is accepted;
* a generation the seat has never reached is refused the same way, so acceptance
  is of *this* occupancy and not merely of "not the predecessor";
* a forged signature fails to authenticate as a seat at all, which is a
  different refusal from a fenced one;
* an ordinary Realm credential carries no seat identity to fence — it must name
  the seat it reports for, and it cannot close a question at all.

## Mutants

Every one compiled and ran; each patch and raw log is retained beside this file
in `mutants/`, and the sources were restored byte-identically afterwards (zero
`MUTANT` markers remain).

| Mutant | What it removes | Outcome |
|---|---|---|
| `M-R2a` | the proved effect latch | killed |
| `M-R2b` | the readback re-proof at binding | killed (after the test's own weak assertion was fixed) |
| `M-R4a` | the claim's pin on the successor occupancy | killed |
| `M-R4b` | convergence on an already-made transition | killed |
| `M-R4c` | the claim's other-key refusal | **survived** — see the frontier below |
| `M-R4c2` | exclusivity itself: the check *and* the uniqueness index | killed |
| `M-R4d` | reconciliation's launch-intent install | killed |
| `M-R4e` | the converger's receipt lookup | killed, with a stated qualification |
| `M-R5a` | content preservation on import | killed |
| `M-R5b` | preserving abridged content under the original digest | killed |
| `M-R5c` | the legacy-generation succession guard | killed |
| `M-R5d` | the occupancy generation in bearer authentication | killed |

## Qualification frontier

Stated rather than implied.

* `M-R4c` (the claim's other-key refusal removed) **survived**: the claim
  read-back added for the occupancy pin refuses the second caller independently.
  `M-R4c2` removes the exclusivity itself — the application check *and* the
  uniqueness index — and is killed. The application check alone is therefore not
  what the barrier test proves.
* `M-R4e` is killed on `status == 200`, not on the receipt-identity comparison:
  `command_receipts` holds `UNIQUE (project_id, idempotency_key)`, so one key can
  never own two receipts and that assertion is a restatement of a constraint. The
  independent content is that a replay after a lost binding reaches 200 at all.
* "The imported key cannot replay or authorize a destination command" is proved
  at the store boundary — no receipt and no succession resolves under it — rather
  than by driving a destination command with it.
* The concurrency is deterministic through one scheduled barrier at the
  retirement. No seam stands inside the store transaction itself, so a loss
  *between* the route write and the ledger write in the same transaction is
  covered by atomicity rather than by a test.


---

# Audit remediation — candidate `9ac33c19`, findings P1×3 and P2×3

Date: 2026-09-29
Audit target: exact `9ac33c196069eae4978068296c30802e0f5e5fd0`
(tree `d986be64b7c56cf85d164d98e641d6d7194c325e`), LSA native 21ad40fb update 325.
Remediation baseline: `a088e037c083b2e6c2c2ddff670df4ea2d918371`.

Everything above is unchanged. The immutable HIGH-VERIFICATION `FAIL` on the old
lineage and the 408e audit are untouched.

## P1 — effect completion proves the whole successor

`native_id` is an external identifier a provider may reissue. Comparing it alone
let a later occupancy stand in for an abandoned succession's own successor and
collect a receipt for work belonging to its successor's successor. Three sites
now agree on what identity means:

* the effect latch fences on the occupancy this command produced and compares
  the successor's recorded runtime generation;
* reconciliation filters on the same pair before it writes anything, so a
  refused command does not leave the earlier occupancy's launch intent installed
  against a later native on its way out;
* the route transition decides "already done" by runtime kind, host, generation
  and id rather than by name.

## P1 — this Realm's own bearers are credential material

`kontor-seat-v1.` and `kontor-seat-v2.` carry no `Bearer ` prefix and matched no
provider marker, so the scanner that refuses every other credential did not know
them. They are now refused *by value*, which is what matters: the key a caller
chooses is the caller's, and after the readback became typed there were no
free-form keys left to forbid anyway.

Separately, a foreign document was believed on its digest. `verify` now rescans
the complete document, embedded JSON included, before an import opens a
transaction — construction scans what this Realm publishes, and this scans what
another Realm hands us.

## P1 — an authentic schema-12 document survives being read

The generation-13 field injected on parse survived into the canonical bytes and
the continuity vocabulary, so a genuine v12 export failed its own digest for
having been read. It is stripped from both below 13.

This is also where the two regressions came from, and both are the same class:
legacy-document *fixtures* in other suites that had silently agreed with the old
strip list. A new export generation therefore touches **eight** places, not six:
the migration, the `migrations.rs` registry and `SCHEMA_VERSION`, `schema_v1.rs`'s
table registry and version assertion, `backup/export.rs`, the
publication-attestation version assertion, the v115 rewind — and now
`backup_export.rs`'s generation-2 fixture and `event_replay.rs`'s generation-7
one.

## P2 — the readback is one strict type

`kontor_core::repository::CoreTeamRouteSuccessionReadback`, `deny_unknown_fields`
throughout, shared by the daemon that builds it, the store that persists it and
the API projection derived from it. Missing and unknown fields are refused where
the document is written, not only where it is read, and the grant-subject digest
is recomputed from the subject it describes rather than carried.

## P2 — a receipt is provably this command's

Migration 0122. The binder joins `command_receipts` inside its own transaction
and requires project, idempotency key, command kind, target epic and intent to
agree; `receipt_id` is unique so one receipt cannot complete two successions;
and `receipted_at` is frozen alongside it.

## Qualification frontier

* **`M-P11a` is a weak kill.** Removing the occupancy fence does not change
  whether the abandoned row is refused — the identity comparison refuses it
  anyway — so the test fails only on the rule it names. A byte-identical native
  identity at two occupancies is unreachable, because
  `hosted_topology_seat_history` is keyed `(project, seat, native_id)`. The
  fence's independent value is against a row written by another path, and this
  suite cannot demonstrate that without planting one. Recorded, not papered over.
* **The daemon-level id-reuse variant is unreachable in the fake.** Its runtime
  holds one generation; advancing it invalidates the container binding snapshot,
  so the request refuses earlier at `ensure_generation` for an unrelated and
  correct reason. The reuse case is proved at the store, which owns the latch and
  the receipt. A fake knob added for this was reverted rather than contrived.
* **Runtime kind and host are not ledger columns**, so the binder proves they are
  *present and well-formed* in the readback, and proves the native id and runtime
  generation against the ledger. Their correctness rests on the write that
  produced them.
* **The `target` and `kind` limbs of the receipt join are defence in depth.**
  `command_receipts.idempotency_key` is unique, so a receipt bearing this
  succession's key cannot also bear another target or kind; only the foreign-key
  case is independently reachable, and that is what the regression exercises.
* **An earlier weak assertion was found and fixed, not preserved.**
  `a_readback_naming_another_successor_is_refused_before_binding` had been
  passing on a foreign-key error and never reached the check it claimed to prove;
  it now presents a genuine receipt. Several store fixtures used partial
  readbacks, exactly as the audit said, and have been rebuilt complete.


---

# Audit remediation — candidate `2ae5ae77`, three P2 findings

Date: 2026-10-02
Audit target: exact `2ae5ae778d209ceb4cd0a09548a9bbd50c048321`
(tree `ec778366452b879afa2af919b28516987064240c`); source baseline
`a088e037` — `2ae5` added evidence only. Mutation baseline for this round:
`41f48e695bcf40498c807ca0a246b5d0c7ddb8a7`.

## P2 — the readback is bound to the transition it commits

A readback could be complete, internally coherent and correctly hashed while
describing a different placement, on a different host, running a different
route, retired at a different instant. Runtime kind, host, provider session and
model route are carried by that document and by nothing else durable, so a field
nobody compares is a field nobody records.

The transition now reads the claim's three generations inside its own
transaction and requires the readback to describe the exact seat rows, grant
generation and retirement instant it is committing. Because the document is
proved before it becomes durable and is immutable after, the effect latch and
the daemon's reconciliation now compare the active seat against *it* rather than
against the two identity columns beside it.

Six rehashed wrong-field cases — successor runtime kind, host, provider session,
model route; predecessor host; retirement instant — each refuse with zero route
movement: no occupant change, no committed route, no history row.

## P2 — the replay path is proved before it answers

The receipt join and the readback verification now run *before* an identical
bound receipt may answer `Unchanged`. Returning it earlier made the idempotent
path the one path that proved nothing, which is backwards: a replay is the most
common way the row is read, so it has to be the most verified.

Migration `0122` additionally asks its own rule of the rows that predate it.
Every already-bound succession must name a receipt demonstrably its own, across
every supported upgrade path, and it fails **closed**: a realm carrying a binding
this generation cannot vouch for does not open. Valid historical bindings cross
untouched — the check reads and writes nothing, and the completion instant is
preserved.

## P2 — the two schema guards are proved independently

`M-P22b` removed uniqueness and the frozen instant together, so its kill could
not say which guard did the work. Both are now separate mutants with separate
tests. Splitting them immediately showed that neither half had been proved; see
the two retained survivals in `mutants/MANIFEST.md`.

## Qualification frontier

* **Two mutants survived their first run, and both found real test defects.**
  `M-P22b1` survived because the second succession was incomplete, so `0120`'s
  completeness rule refused before uniqueness was consulted. `M-P25` survived
  because the test bound an *unbound* row and never reached the branch whose
  order had changed. Both records are retained beside the kills.
* **The binder's ledger-identity comparison is now double-guarded on the live
  path.** With the readback bound to its transition, a disagreeing document can
  no longer be committed, so `verify_succession_readback` is reachable only for a
  row written by another path — which is what
  `a_replayed_binding_is_proved_before_it_answers_unchanged` plants.
* **The upgrade validation fails closed rather than quarantining.** No quarantine
  table was designed, because a realm that cannot vouch for a binding should stop
  rather than carry a second, weaker class of evidence.
* **Runtime kind and host remain absent from the ledger.** They are now compared
  against the *transition* at write time and against the stored readback
  thereafter; nothing re-derives them from an independent durable source.
* Carried forward unchanged: `M-P11a` is a weak kill; the daemon-level id-reuse
  variant is unreachable in the fake.


---

# Additive join with current master — joined head `7646bcd9`

Date: 2026-10-03
Authorization: operational `44d5d916-d778-46b8-b00d-2cca01cc6553`
("8187-additive-join"); resumed under `kontor-closeout-20261003`.
Everything above is unchanged.

## What the join is

`aa1ae38c` is a true two-parent merge — first parent `99b23720` (the audited
candidate), second parent `8acdbd17` (current default) — verified by reading the
parent list, not merely by ancestry. `408e7895` remains an ancestor through
`99b23720`.

Exactly one conflict, the known one, in `crates/kontor-store/src/lib.rs`. It was
adjacency rather than disagreement: both sides appended methods to the same
`impl SqliteStore` block and only the closing brace collided. The resolution is
the complete union — the fault-injection cells and all three arming methods from
this lane, the read-only `read_existing_realm` from the default. The two do not
interact: `open` initializes connection, realm and conditionally faults, while
`read_existing_realm` returns Realm metadata without constructing a store, so it
needs no fault state. `migrations.rs` auto-merged and keeps both sides.

## The one interaction the earlier audits could not have covered

ASMA-8015's read-only operator preflight arrived while this lane was taking the
schema from 119 to 122. `verify_applied` requires an exact `user_version` match,
so on a joined 122 binary a Realm stopped at 119 is **refused** — it is not
migrated. That is the designed behaviour, and the tests added here prove the
refusal stays read-only: bytes unchanged, `user_version` still 119, no migration
table recreated.

The consequence, stated as a question for the LSA rather than decided here:
raising 119→122 widens the set of "legacy" Realms by three versions, so an
operator holding a Realm stopped at 119, 120 or 121 must start the daemon once
to migrate before `install-jira-credential` will accept it. Refusal surfaces as
`OperatorError::CredentialScope`. Whether that is acceptable is a compatibility
decision; it was **not** addressed by relaxing validation.

## Contract checks at the joined head

The generated OpenAPI contract and the console schema are byte-identical to the
candidate parent, and neither incoming default commit touches either path, so
the join introduced no generated API difference. `openapi.json` and
`schema.d.ts` both match `99b23720` exactly; regenerating the console schema
from the committed contract reproduces the committed file byte for byte
(`2157df18…`). The difference against `173d399b` is pre-existing ASMA-8187 work
already covered by the earlier audits, not drift from this join.

## Qualification frontier

* Everything carried forward from the `99b23720` round still stands: `M-P11a` is
  a weak kill; the daemon-level id-reuse variant is unreachable in the fake; the
  binder's ledger-identity comparison is double-guarded on the live path.
* The stopped-Realm compatibility consequence above is a **decision owed**, not a
  defect closed. This lane proved the refusal is safe; it did not and should not
  decide whether the workflow needs pre-122 Realms.
* Two `git add` invocations were used to stage before the ASMA commit flow. The
  authorization permitted the single merge command and ASMA explicit-path commit
  completion; raw staging was not named. Disclosed in the procedure record.
