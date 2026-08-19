# KON-OP-05 implementation

Date: 2026-08-17
Architecture: `docs/evidence/KON-OP-05/ARCHITECTURE.md` (approved for
implementation)
Checkpoint reached: **1 of 4**

## What is composed

Checkpoint 1 of the architecture's four composition checkpoints: the typed
specifications, their immutable publication, and the six Admin/read operations
that carry them. `cargo test --workspace` is green (1438 tests), as are
`cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check`.

### Specifications — `kontor-core::consultation`

`AdvisorProfileSpec` and `CommitteeTemplateSpec`, each with `validate()` and
`canonicalize()`, following the `CalendarProfileSpec` idiom. Closed enums for
`ConsultationFamily`, `MemoryAccess`, `ConsultationScope`, `CommitteeRole`,
`AggregationProtocol`, `DiversityRule`, `CommitteeVerdict` and
`AdviceDisposition`. `conjunctive_outcome()` is the settlement rule as a pure
function of the durable findings.

Neither specification can describe a mutation: there is no capability list,
operation allowlist, scheduler hook or gate waiver field anywhere in them.
`AggregationProtocol` admits only `conjunctive`, so a template cannot name a
deferred protocol and silently receive a conjunction.

### Persistence — migration `0034`

The feature branch originally occupied `0032`; integration placed it after the
canonical OP-04 schema as `0034` without changing its data contract.

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
2. **`DiversityRule` is declared template data.** The architecture mandates
   test-only two- and five-seat templates; requiring pairwise-distinct providers
   unconditionally would demand five providers for the five-seat fixture.
   `independent_review@1` declares `distinct_provider_per_slot`.
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

- `kontor-core/tests/consultation_specs.rs` (22) — the conjunctive truth table
  including a missing finding blocking settlement and incomplete evidence
  settling `NON_COMPLIANT`; diversity over the whole chain; cardinality as data
  at two and five seats; one reviewer, two Judges, duplicate slot ids and a third
  round refused.
- `kontor-store/tests/consultation_profiles.rs` (9) — version one, consecutive
  versions, a refused gap, a refused republish, independent versioning per
  profile, the two families as separate catalogs, byte-for-byte read-back and
  project scoping.
- `kontor-profiles/tests/consultation_presets.rs` (5) — the shipped preset parses,
  validates, is the only one, and actually exercises the diversity rule.
- `kontor-daemon/tests/loopback_api.rs` (+8) — preview commits nothing; publish
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
