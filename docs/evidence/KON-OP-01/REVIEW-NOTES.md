# KON-OP-01 / ASMA-7870 — review notes

Date: 2026-08-16
Commits reviewed: `dedd300`, `597fa26`
Reviewer: OP-01 builder seat (self-review before handoff)

## Verdict

**READY-FOR-QA.** One finding was raised and fixed during the change; it is
recorded below rather than silently absorbed.

## Findings

### F-1 — the publish boundary's stamp validation was unexercised (fixed)

Severity: P2 · Status: fixed in `597fa26`

Mutation M6 removed `shareability.validate_for(...)` from both publish paths and
the whole suite stayed green. The insert trigger only proves that an override
names a human; a *forged* stamp — `kontor_local` class wearing the type-default
rule's identity — is well-formed as far as SQLite is concerned. Nothing tested
that the domain refuses it, so the one check standing between "a human withheld
this" and "it was withheld and nobody knows by whom" was load-bearing but
unproven.

Fixed by `publishing_refuses_a_class_nobody_chose`
(`crates/kontor-store/tests/operational_topology.rs:355`), which asserts both
publish paths refuse the forged stamp and that a refused publish leaves nothing
behind. Re-running M6 now kills it.

## Points checked

- **Tier A cannot be classified anywhere.** Refused in the domain constructors
  (`spec.rs:165`) and given no column to occupy (`schema_v1.rs:3038` asserts zero
  `shareability%` columns on all three tier-A tables). Both layers, not one.
- **Immutability is real, not conventional.** Reclassification is refused by the
  `0023` `BEFORE UPDATE` triggers, proved through direct SQL at
  `operational_topology.rs:284` rather than through the repository that would
  never attempt it.
- **A default exists for every classifiable tier**, so ordinary work never stalls
  on a human — the explicit OP-REQ-037 requirement. Proved at
  `spec_validation.rs:1630`.
- **No decision is attributed to a human who did not make one.** The backfill
  writes `type_default` with a NULL classifier (`schema_v1.rs:2952`), and an
  override with no identity is refused by the schema
  (`operational_topology.rs:316`).
- **The canonical hash is unchanged** by classification, so no pinned
  `(spec_id, revision, canonical_hash)` moved and no existing fixture changed.
- **Read path re-proves the pairing** (`repository.rs:916`), so a row edited
  around the repository cannot read back as a valid stamp.
- **No scope creep.** No publisher, synchronizer, file writer, importer,
  conflict resolver or drift detector; no `/v1`, MCP or CLI surface. Confirmed by
  the changed-file list, which is wholly inside OP-01's declared ownership.

## Deliberate simplifications

- `provenance` is stored explicitly rather than derived from `classifier`, even
  though the two are correlated. OP-REQ-037 names all three facts, and the DB
  enforces their agreement independently of the Rust constructors.
- Read-back of the stamp is two additive ports rather than a change to the
  existing `get_topology_spec` / `get_role_catalog` return types, which would
  have churned callers outside OP-01's ownership for no gain.

## Not reviewed here

OP-REQ-036 (`NEEDS_HUMAN` payload and the typed MCP refusal) is OP-03's
ownership per the plan's OP-03 Implementation paragraph; this ticket adds no
escalation surface, notification transport or prompt channel, which is the only
OP-REQ-036 obligation that falls on a domain/store ticket.
