# KON-OP-05 Checkpoint 1 code review

Date: 2026-08-18
Status: passed; six findings, none blocking
Scope: the Checkpoint 1 composition in `e338b9d`, reviewed against
`ARCHITECTURE.md`
Seat: inspector (`code` work profile, `code-review` phase)

Companion to `ARCHITECTURE.md` and `IMPLEMENTATION.md`. That pair decides and
records; this one reports what the build does under failure, which is the part
the happy-path tests do not reach.

## Verdict

No blocking defects. The checkpoint does what it says: two specifications that
cannot describe a mutation, one immutable table both families share, six
operations that publish exactly one revision under one receipt and seat nobody.
The proofs are real — they drive the public API, and each one fails for a
distinct reason.

The findings below are all recoverable or inherited. The most valuable of them
is not OP-05's fault at all: the command-receipt table has been carrying no
immutability triggers since schema v10, and migration `0032` is the eighth
rebuild to drop them.

## Findings

### 1. `command_receipts` has had no immutability or no-delete trigger since v10

`0001_init.sql:1723` creates two triggers on `command_receipts`:
`command_receipts_identity_immutable`, which refuses any update that rewrites
`idempotency_key`, `target`, `intent`, `intent_hash`, `kind` or `project_id`, or
that touches a receipt already `confirmed`/`failed`; and
`command_receipts_no_delete` (`:1733`).

Every migration that widens the `kind` CHECK rebuilds the table — v10, v12, v24,
v28, v29, v30, v31 and now `0032_consultation_profiles.sql:71-126`. `DROP TABLE`
drops a table's triggers with it, and each rebuild re-creates
`ix_command_receipts_state` and nothing else. Neither trigger has been
re-created since.

Verified, not inferred: a freshly migrated database at v32 reports zero rows for
`SELECT name FROM sqlite_master WHERE type='trigger' AND tbl_name='command_receipts'`.
Nothing in `schema_v1.rs` would have caught it — the suite counts
`account_profiles%` triggers after an upgrade (`:311`) but asserts nothing about
this table.

Nothing in the build deletes or rewrites a receipt, and `ensure_replay`
(`applications.rs:1715`) enforces the same rule in application code. So this is
lost defence in depth rather than live corruption — but it is exactly the
guarantee OP-05 leans on when it says a key plus an intent returns the original
projection, and the OP-04 review recorded the rebuild precedent as something
these migrations "follow exactly". The precedent is the bug.

Fix: re-create both triggers at the end of the next command-receipt rebuild,
beside the index that is already re-created there, and pin the pair in
`schema_v1.rs` so the ninth rebuild cannot drop them silently.

### 2. An apply whose receipt never landed is refused as a stranger's edit

`apply_consultation_profile` publishes the revision (`applications.rs:2976`) and
records the receipt afterwards (`:3008`). The comment at `:2940` says replay is
judged before the expected revision so that "a retry after a lost
acknowledgement replays instead of being refused for having succeeded". That
holds for a lost *response*. It does not hold for a failure between the two
store calls — `with_store` is a mutex around one call, so they are two
transactions.

In that window the row is durable and no receipt exists. The retry finds no
receipt, takes the not-replayed branch, reads a catalog that has moved by one,
and is refused `revision_conflict` — "the profile catalog moved since the caller
read it", naming the caller's own successful write as somebody else's edit.

Confirmed by probe: apply the same document twice under different keys (the same
branch, since `replayed` finds nothing either way and the key is not read again
before the refusal) and the second call returns `409 revision_conflict` with
`current_revision: 2`.

The blast radius is small — the publication is durable, immutable and
non-duplicable, since the store's per-profile gap check and the primary key both
refuse a second write of that version — so an Admin who re-reads the catalog
sees their revision and moves on. What is actually lost is the receipt: a
published policy document with no durable record of who published it.

The fix is *not* receipt-first. A receipt written before a failed publish would
be permanent, and every retry would then skip the write and die on "the
published revision could not be read back" (`:3005`) — a poisoned key with
nothing published, which is worse. Reconcile on the durable row instead: before
the revision check, if `(family, profile_id, version)` already exists with this
canonical hash, treat it as the completed write, fall through to read-back and
record the receipt.

Inherited shape: `apply_core_team` (`:6652-6672`) has the same window. Worth
fixing in one place for both.

### 3. Provider diversity became opt-out, on a thinner reason than it reads

`DiversityRule::None` is publishable template data (`consultation.rs:114`), so
independence is declared rather than structural. `independent_review@1` declares
`distinct_provider_per_slot`, and the architecture only ever demanded the rule
of that preset — so nothing is wrong today.

The justification in `IMPLEMENTATION.md` (decision 2) is that enforcing
distinctness unconditionally "would demand five providers for the five-seat
fixture". `ProviderRef` is an open key: `consultation_specs.rs:232` builds the
five-seat fixture from `chain(&["anthropic"])` five times when five distinct
names would have cost nothing at the specification layer. The real constraint is
that a five-reviewer *run* would need five providers the runtime catalog
actually has — which is a Checkpoint 3 problem, not a validation one.

Not blocking, because no Committee convenes yet. Worth re-deciding before CP3
makes it real: as it stands, a future template can seat two reviewers on one
provider and present the conjunction as an independent review.

### 4. A closed document with no published shape, discovered one violation at a time

`preview_consultation_profile` returns at most one violation — `validate()`
returns on the first error, and `:2880` wraps that single `Err` in a one-element
vector. The architecture asks for "stable violations" (plural), and apply's
refusal is deliberately and correctly detail-free, pointing the caller back at
preview.

Meanwhile the shape is not published anywhere: `ProfilePreviewRequest.definition`
stays `serde_json::Value` on the wire, and the MCP tool spec types it
`ArgType::Json`, "The complete candidate definition"
(`kontor-mcp/src/registry.rs:2992`). Both are consistent with decision 6 — no
DTO change was needed for these six operations — but the combination means an
Admin publishing a fifteen-field document over MCP discovers it by serial
guessing, one field per round trip.

Cheapest fix is the violation list: collect rather than short-circuit. Publishing
the specs as OpenAPI schemas is the larger, optional half.

### 5. Nothing checks that a declared role can ever hold the seat

`allowed_caller_roles` and `CommitteeSlotSpec.logical_role` are `RoleKey`s, and a
seat's role is a `CatalogRoleRef`/`RoleCode`. The bridge between them is
`OperationalDelivery::role_code()`, which returns `None` for an unbound role
precisely so a caller "refuses rather than inventing a code"
(`kontor-profiles/src/pack.rs:270`).

Neither specification consults it, and the daemon has it in hand at preview time
(`self.domain.delivery`, used at `applications.rs:5925` and `:6857`). So a
profile naming a role no binding covers publishes cleanly, is immutable, and
first fails at CP2/CP3 invocation — as an unconvenable revision that cannot be
edited, only superseded.

The shipped preset is fine: `architect → SA`, `reviewer → AUD`, `judge → AUD`.
This is about the guard, not the data. The slot side matters more than the caller
side, since every slot must become a `NewSeatBinding` in CP3.

### 6. Coverage gaps

- **Interrupted apply.** No test drives a failure between publish and receipt.
  This is the same gap OP-04's review named, and remediation there closed it with
  focused recovery tests; the lesson did not carry forward.
- **Unknown project.** `consultation_catalog` resolves the project first, with a
  comment explaining why an unknown project must not read as an empty catalog
  (`:2842`). Nothing tests it — no `404` assertion exists anywhere in the OP-05
  block.
- **The Judge cannot flip the conjunction.** `conjunctive_outcome` ignores slots
  outside `required`, so a Judge aggregate cannot turn a dissent into
  `Compliant`. That is an architecture-required proof and the function is already
  here; one assertion in `consultation_specs.rs` would pin it today rather than
  in CP3.
- **The five run stubs.** No test in any test crate mentions `advisor-runs` or
  `committee-runs`. Narrowing
  `every_successor_contract_refuses_rather_than_answering_emptily` to the
  Completion routes was honest — both catalogs are composed now, and
  `an_unpublished_consultation_catalog_is_empty_and_says_so` replaces the rule it
  protected — but it leaves the family with no "does not pretend" coverage at
  all, inherited from OP-03, at the checkpoint before CP2 starts filling those
  stubs in.

## What holds

Recorded because these were the parts most likely to be got wrong, and were not:

- the specifications genuinely cannot name a mutation — no capability list,
  operation allowlist, scheduler hook or gate waiver field exists to name one
  with, and `AggregationProtocol` admits only `conjunctive`, so a template cannot
  claim a deferred protocol and receive a conjunction;
- preview commits nothing — no draft, no id, no receipt — and the catalog is
  provably still empty afterwards;
- the family comes from the route and the preview hash is bound to project and
  family, so a preview taken against one catalog cannot authorize a publish into
  the other;
- the definition is stored as the canonical document's exact bytes and re-admitted
  through `CanonicalDocument::from_stored` on read, so a rewritten row is refused
  rather than served to a run that pinned the original;
- immutability and permanence triggers on `consultation_profile_revisions` match
  the `core_team_revisions` precedent, and the store's gap check is per
  `(project, family, profile_id)` with the primary key behind it, so two
  concurrent publishes of one version cannot both land;
- `conjunctive_outcome` returns `None` for a missing finding rather than
  counting absence as agreement, and a recorded finding with incomplete evidence
  stays in the denominator;
- diversity is judged over the whole model chain, which is the right call and is
  the one place a fallback rung could have collapsed independence under load;
- the migration widens the command-kind CHECK by the two publication commands
  only, and `domain_state.rs` pins both to the project aggregate;
- publishing seats nobody, proven against `topology:inspect` before and after;
- authority is Observer to read and Admin to preview or publish, and an Operator
  bearer is refused before the service is reached.

## Evidence

- `cargo test --workspace --no-run` — clean.
- `cargo test --workspace` — 113 suites, 1438 passed, 0 failed, 8 ignored. The
  `kontor-cli/tests/memory_parity.rs` loopback-bind failure OP-04's review hit in
  the sandbox did not recur here.
- `cargo clippy --workspace --all-targets -- -D warnings` — clean.
- `cargo fmt --check` — clean.
- Two throwaway probes, run and reverted: the interrupted-apply refusal in
  finding 2 (`409 revision_conflict`, `current_revision: 2`), and the empty
  trigger list on `command_receipts` in finding 1.

## Gate

`code-review-gate`: **passed**. Nothing here blocks Checkpoint 2. Findings 2 and
5 should land before the run operations are composed on top of them — one is an
audit hole in the receipt trail, the other decides whether an unconvenable
profile is publishable. Finding 1 belongs to the next command-receipt rebuild,
whichever checkpoint owns it. Finding 3 is a decision to re-take before CP3, not
a change to make now.
