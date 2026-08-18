# KON-OP-05 implementation

Date: 2026-08-17
Architecture: `docs/evidence/KON-OP-05/ARCHITECTURE.md` (approved for
implementation)
Checkpoint reached: **1 of 4** (review findings closed)

## What is composed

Checkpoint 1 of the architecture's four composition checkpoints: the typed
specifications, their immutable publication, and the six Admin/read operations
that carry them. `cargo test --workspace` is green (1446 tests), as are
`cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check`.

### Specifications — `kontor-core::consultation`

`AdvisorProfileSpec` and `CommitteeTemplateSpec`, each with `validate()` and
`canonicalize()`, following the `CalendarProfileSpec` idiom. Closed enums for
`ConsultationFamily`, `MemoryAccess`, `ConsultationScope`, `CommitteeRole`,
`AggregationProtocol`, `CommitteeVerdict` and `AdviceDisposition`. `conjunctive_outcome()` is the settlement rule as a pure
function of the durable findings.

Neither specification can describe a mutation: there is no capability list,
operation allowlist, scheduler hook or gate waiver field anywhere in them.
`AggregationProtocol` admits only `conjunctive`, so a template cannot name a
deferred protocol and silently receive a conjunction.

### Persistence — migration `0032`

`consultation_profile_revisions`, one table for both families discriminated by
`family`, with immutability and permanence triggers. The definition is stored as
the canonical document's exact bytes and re-admitted through
`CanonicalDocument::from_stored` on read, so a rewritten row is refused rather
than served to a run that pinned the original. Version one starts a profile and
every later version must be exactly the next one.

There is no `is_current` column. A profile's current revision is the highest
version published under its id, and a family's catalog revision is the number of
revisions published into it — both derived from the rows.

The migration widens the command-kind CHECK by `apply_advisor_profile` and
`apply_committee_template` only. The invoke, findings and settle commands are not
accepted by the database yet, because no service writes them.

### Operations

`GET`/`:preview`/`:apply` for both `advisor-profiles` and `committee-templates`
are composed on the durable service. Preview is a pure read that commits no
draft, receipt, id or aggregate. Apply revalidates the document, compares the
expected catalog revision, holds the caller to the preview hash it was shown, and
publishes one immutable revision under one receipt. Replay is judged before the
expected revision, as in `apply_core_team`, so a retry after a lost
acknowledgement replays instead of being refused for having succeeded.

The family comes from the route, never from the document.

### Seed

`crates/kontor-profiles/fixtures/consultation-presets.json` ships
`independent_review@1` and nothing else — two reviewers on contrasting providers
and one Judge, conjunctive, `round_limit: 2`. It is data, loaded through the same
parser a deployment would use for a pack of its own, per the `seeds.rs` rule that
behavioural names live in JSON and not in Rust. Publishing it is still an Admin
apply against a project.

## Decisions taken during implementation

1. **Diversity is judged over the whole model chain, not the primary rung.** A
   shared fallback would collapse reviewer independence precisely under load.
   `reviewers_colliding_only_on_a_fallback_rung_are_refused` fails if only
   primaries are compared.
2. ~~**`DiversityRule` is declared template data.**~~ **Reversed** by review
   finding 3. Provider distinctness among reviewers is now structural and cannot
   be switched off; `DiversityRule` is deleted. The original justification was
   thin: `ProviderRef` is an open key, so the five-seat fixture costs nothing by
   naming five distinct providers, and whether a five-reviewer *run* can find
   five providers the runtime catalog has is a launch-time question, not a reason
   to let publication claim independence it does not have.
3. **Approved memory is an access level, not a list of record ids.** Approval
   already lives on the memory record; enumerating ids would freeze another
   aggregate's snapshot and drift on the first tombstone.
4. **`ProviderRef` is the provider-family identity.** The architecture's
   "contrasting provider families" needed no new type — `ProviderRef` is already
   the runtime catalog's provider id.
5. **Violation detail travels only through `preview`.** `ApiError` carries a
   `&'static str` by construction, so an apply refusal is deliberately
   detail-free and points the caller at the preview whose `violations` are typed
   for it. Document text cannot ride out in an error.
6. **No OP-03 DTO change was required for these six operations.** Deserializing
   `definition` into the route's family type, with `deny_unknown_fields`, delivers
   the mandated correction inside the daemon; the wire contract and OpenAPI are
   untouched. The remaining DTO corrections (closed invoke scope, tagged
   findings, family-specific settlement, richer projections) belong to the run
   operations and are Checkpoint 2–4 work.

## Proofs

- `kontor-core/tests/consultation_specs.rs` (23) — the conjunctive truth table
  including a missing finding blocking settlement and incomplete evidence
  settling `NON_COMPLIANT`; diversity over the whole chain; cardinality as data
  at two and five seats; one reviewer, two Judges, duplicate slot ids and a third
  round refused, and a Judge aggregate unable to overturn a dissent.
- `kontor-store/tests/consultation_profiles.rs` (9) — version one, consecutive
  versions, a refused gap, a refused republish, independent versioning per
  profile, the two families as separate catalogs, byte-for-byte read-back and
  project scoping.
- `kontor-profiles/tests/consultation_presets.rs` (5) — the shipped preset parses,
  validates, is the only one, and actually exercises the diversity rule.
- `kontor-daemon/tests/loopback_api.rs` (+13) — preview commits nothing; publish
  and read back; version two against the reported revision; replay publishes
  once; a swapped definition under a valid hash refused; a stale revision writes
  nothing; an unknown field reported rather than dropped; an Advisor profile
  refused by the Committee route; publishing materializes no topology node.

`every_successor_contract_refuses_rather_than_answering_emptily` now covers only
the Completion routes. An empty consultation catalog is a truthful answer — a
project that has published no profile has none, and the reported revision says
the catalog is untouched — which is not the case that guard existed to prevent.

## Not implemented

Checkpoints 2, 3 and 4. The five Advisor/Committee run operations remain typed
`Unavailable` stubs, and `Services::resolve_scope` still refuses
`AdvisorConsultation` and `CommitteeConsultation`. That refusal is still correct:
no durable run resolves their epic yet.

Specifically outstanding: frozen context provenance and the `ResolvedContextPack`
wiring; the semantic effect adapter and one-ASW/one-seat placement through OP-02;
Advisor output and disposition; Committee invocation, independent findings, Judge
ordering and server-recomputed settlement; remediation round lineage,
`NEEDS_HUMAN`, and restart reconciliation. `TSC`→`CSW` normalization is already
provided by `kontor_core::id` and needs no further work at the id layer.

Checkpoint 1 was committed on its own because the architecture requires each
checkpoint to build and forbids enabling a route on an in-memory aggregate,
unvalidated JSON or fake success while its durable composition is incomplete.

## Review findings — dispositions

`REVIEW.md` passed Checkpoint 1 with six non-blocking findings. All six are now
closed, ahead of CP2, because the review sequenced two of them that way: the
receipt-trail reconciliation had to land "before CP2 composes run behavior on
that receipt trail", and the diversity rule had to be re-decided "before CP3
makes it real".

1. **`command_receipts` lost its triggers at v10 — fixed.** Migration `0033`
   re-creates `command_receipts_identity_immutable` and
   `command_receipts_no_delete`. It deliberately does *not* rebuild the table:
   rebuilding is what dropped them through eight successive migrations.
   `schema_v1.rs` now pins both by name and proves they refuse a `DELETE` and an
   identity rewrite while still allowing a state advance, so a ninth rebuild
   fails the suite instead of shipping.
2. **An interrupted apply is reconciled — fixed.** `apply_consultation_profile`
   now checks, before the revision check, whether this exact
   `(family, profile_id, version)` already carries this exact canonical hash. The
   table is immutable, so nobody else can have put it there: it is the caller's
   own completed write. It falls through to read-back and records the receipt
   that was lost. Receipt-first was rejected for the reason the review gives — a
   permanent receipt over a failed publish poisons the key.
3. **Provider independence is structural — fixed, reversing decision 2.**
   `DiversityRule` is gone. Two reviewers who can reach one provider, on any rung,
   are refused at publication. The five-seat fixture now names five distinct
   providers.
4. **Violations are collected, not discovered one at a time — fixed.** Both
   specifications expose `violations()` returning every reason at once;
   `validate()` is defined as the first of them, so the two can never disagree.
   A public test asserts three independent faults arrive together. Publishing the
   specs as OpenAPI schemas — the larger, optional half — is not done.
5. **Unbound roles are refused before publication — fixed.** The daemon checks
   every declared role, caller side and slot side, against
   `OperationalDelivery::role_code`. The check lives in the daemon because only it
   holds the binding. A profile naming a role no binding covers is now a preview
   violation rather than an immutable, unconvenable revision.
6. **Coverage gaps — closed.** New tests cover the interrupted-apply retry, an
   unknown project refusing rather than reading as an empty catalog, the Judge
   being unable to turn a dissent into `COMPLIANT`, and all five consultation run
   operations refusing without carrying a receipt, state or verdict.

One finding is noted and not closed: `apply_core_team` has the same
publish-then-receipt window (review finding 2, "inherited shape"). It belongs to
OP-04's aggregate and is left untouched rather than widened into this change.

## Release gate

`RELEASE-NOTES.md` rejects the release, correctly: the admitted OP-05 task is
working Advisor and Committee consultations, and Checkpoints 2–4 remain
unimplemented. Closing these findings does not change that verdict. It removes
the preserved blockers that the release note lists as "additional release
blockers to close during completion", so that CP2 composes run behaviour on a
receipt trail that reconciles and on definitions that cannot name an unseatable
role.
