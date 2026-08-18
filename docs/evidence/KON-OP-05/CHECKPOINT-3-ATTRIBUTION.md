# KON-OP-05 CP3 — member-slot attribution: options for decision

Date: 2026-08-18
Author: builder seat
Status: **ratified — Option B** (Igor, via TPM, 2026-08-18)

CP3 composes Committee findings. Every finding must be attributable to the exact
frozen role slot that produced it, because the conjunctive rule measures
*agreement between independent readers*. Attribution is therefore not
bookkeeping: it is the property the verdict's meaning rests on.

## The problem

The realm authenticates by tier, not by principal: three bearer secrets —
observer, operator, admin — and `Caller` carries only the resolved tier
(`kontor-api/src/lib.rs:78`, `auth.rs:70`). There is no identity at the boundary
to attribute a finding to.

The requester semantic was settled by deriving the owner authority from durable
state (the epic's ECP LSA). **That mechanism cannot be reused here.** There is one
owner per epic and several member slots per Committee, so a derivation that always
returns the same seat cannot distinguish reviewer A from reviewer B.

Deriving the slot from a request body field is worse than useless:

> One Operator credential submits reviewer A's finding as `COMPLIANT` and
> reviewer B's finding as `COMPLIANT`, and the server settles `COMPLIANT` and
> reports it as an independent review. Nothing in the record shows the two
> findings came from one place.

That is the precise lie the whole design exists to prevent, so a body-supplied
slot is not on the table.

## Options

### A. Per-slot submission materials

Mint a bounded, single-purpose credential per frozen slot at invoke, delivered
only into that slot's seat. A finding authenticates with it and the server derives
the slot from the material presented.

- *Prevents:* cross-slot forgery — a caller holding reviewer A's material cannot
  produce reviewer B's finding.
- *Costs:* introduces per-principal credential material, which this realm has
  deliberately never had. Needs minting, secret-at-rest handling, rotation, and
  revocation when a seat is retired or replaced. Every one of those is a new
  security surface inside OP-05's boundary.

### B. The runtime session's bound seat proves the submitter (recommended)

A finding is accepted only through the runtime session Kontor bound to that seat,
and the daemon attributes it from the durable `SeatBinding` ↔ observed native
binding, never from anything the caller says.

- *Prevents:* the same forgery, because the proof is *where the finding arrived
  from* rather than what it claims to be.
- *Costs:* a finding cannot be recorded before the seat is genuinely attached, so
  settlement is coupled to OP-02's readback evidence; a lost or replaced session
  needs an explicit re-attach path rather than an implicit one.
- *Why recommended:* it mints no secrets, reuses evidence the platform already
  produces, and makes attribution structural. It also fails in the safe direction:
  an unattested session cannot record a finding at all, which blocks a round
  rather than admitting an unattributable verdict.

### C. Server-driven collection instead of inbound submission

The daemon asks each seat for its finding and records the answer. Attribution is
structural by construction: the response belongs to the seat that was asked.

- *Prevents:* forgery entirely — there is no inbound findings route to forge.
- *Costs:* inverts the OP-03 wire contract, which fixed `findings:record` as an
  inbound operation; needs a collection and timeout mechanism; couples settlement
  to runtime liveness more tightly than B.
- *Note:* architecturally the cleanest of the three, and the most expensive to
  adopt now because it changes an agreed contract rather than filling it in.

## Recommendation

**Option B**, with A as the fallback if session binding turns out too weak to
attest, and C recorded as the design that should be reached for if the inbound
contract is ever reopened.

## What this seat is doing meanwhile

Not composing `findings:record`. Everything in CP3 that does not depend on
attribution — durable Committee runs, the frozen template's declared slots, one
CSW with those exact seats, provider-diversity and placement preflight, round
lineage, and the server-recomputed conjunction over whatever findings exist — can
be built behind whichever option is chosen, and the settlement rule is already a
pure function (`conjunctive_outcome`) with its truth table proven.

Recording rather than choosing, for the same reason as the requester semantic: an
attribution rule is inherited by every finding ever recorded, and a wrong one is
not visible in the evidence it produces.

## Ratified disposition (Igor, via TPM, 2026-08-18)

**Option B — bound-seat proof.** Member-slot attribution comes from the durable
`SeatBinding` ↔ observed native binding: the proof is *where the finding arrived
from*, never what the request claims. It mints no secrets, reuses evidence OP-02
already produces, and fails safe — an unattested session blocks a round rather
than admitting an unattributable verdict.

Recorded with the same care as the requester semantic, and meaning the same kind
of thing: a finding's slot attribution is **a citation of which seat produced it**,
not a claim about which human or operator typed it. The realm authenticates by
tier, so no record here identifies a person, and none may be read as doing so.

Consequences this seat will implement:

- a finding is accepted only through the runtime session bound to the frozen slot's
  seat, and the slot is derived server-side from that binding;
- `findings:record` never reads a slot, member identity or principal from the
  request body — the field does not exist to be sent;
- a seat whose native binding has not been observed cannot record a finding at all.
  The round waits, visibly, rather than accepting evidence nobody can attribute;
- retiring or replacing a seat mid-round therefore ends that seat's ability to
  submit, which is the intended behaviour: the replacement is a different reader.

Options A (per-slot submission materials) and C (server-driven collection) remain
recorded above. A is the fallback if bound-seat proof turns out too weak to attest;
C is the design to reach for if the inbound `findings:record` contract is ever
reopened.
