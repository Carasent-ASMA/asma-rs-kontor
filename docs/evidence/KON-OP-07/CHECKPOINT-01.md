# KON-OP-07 checkpoint 1 — project/subject authority ledger

Date: 2026-08-17
Status: implemented; every gate green
Against: `docs/evidence/KON-OP-07/ARCHITECTURE.md` (checkpoint 1)

## What landed

Authority is now a fact about `(project_id, subject)`; the realm singleton is
unreachable.

- `kontor_core::authority` — closed `AuthoritySubject`, `SubjectOrigin`,
  `SubjectAuthority` and the `ProjectSubjectAuthority` row. Origin (immutable,
  recorded at creation) is kept separate from authority (who may write now).
- Migration `0032_project_subject_authority.sql` — `project_subject_authority`,
  `subject_import_manifests`, `subject_authority_receipts`; table CHECKs make a
  native row carrying cutover evidence and a partially-switched row
  unrepresentable; one trigger permits exactly two updates (attest, switch) and
  nothing else; `memory_authority` is frozen by trigger rather than dropped.
  Existing projects are seeded: backlog native, memory from what the singleton
  claimed.
- `kontor_store::authority` — `require_subject_authority` (inside the caller's
  transaction), ledger reads, `attest_subject_source_frozen`,
  `record_subject_import`, `switch_subject_authority`, receipts.
- Memory writes (propose, approve, tombstone, purge) now check
  `(project, memory)`. `memory_readback_hash` is computed from stored rows;
  `switch_project_memory_authority` recomputes it and the ledger refuses a switch
  whose recomputation disagrees with what the import recorded.
- `projects:ensure` requires both origins, carries them in its idempotency intent,
  reports both subjects back, and refuses a re-ensure that states different ones.
- `/v1`: `GET /v1/projects/{id}/subjects/authority`,
  `POST /v1/projects/{id}/subjects/authority:attest`, `cutover:switch` now takes
  `expected_revision`, and the realm-wide `cutover:freeze` is routed only to
  refuse with the name of its replacement. Two MCP tools added
  (`kontor_subject_authority_get`, `..._attest`).

## Proofs

Every gate `CONTRIBUTING.md` names: `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace --no-fail-fast`, `cargo deny check`,
`pnpm --filter kontor-console verify:api`, `pnpm -r typecheck`, `pnpm -r test`.

The contract document and the console types generated from it are regenerated in
the same change, because `verify:api` diffs the committed `schema.d.ts` against a
fresh generation: a DTO that gains a field and a console compiled against the
contract before it are the same commit or neither.

- `migration_0032_seeds_a_populated_v31_realm_from_what_the_singleton_claimed` —
  a genuine populated pre-authority file upgrades: backlog seeds native because
  its graph already lived here, memory inherits the realm-wide claim, and no
  cutover evidence is invented for a claim this migration did not witness.
  Mutation-checked: seeding backlog from the singleton too turns it red.
- `migration_0032_seeds_existing_projects_and_guards_the_ledger` — the singleton
  refuses UPDATE; authority never returns to the legacy system; a native row
  cannot carry switch evidence; three partial-switch shapes abort; no delete.
- `cutover_is_attested_hashed_transactional_and_idempotent` — pending subject
  refuses native writes; a failure *inside* the item loop rolls its items back;
  re-import is idempotent; switch refuses without the
  attestation and against a readback that does not describe stored state;
  authority moves exactly once; a sibling project in the same realm is
  unaffected; the memory switch leaves that project's backlog untouched.
- `loopback_api` — a fresh project is writable on both subjects immediately;
  re-ensure with a different origin is 409; the realm-wide freeze refuses.
- `memory_parity` — the empty-export/freeze/switch ceremony a fresh project used
  to perform is deleted, not replaced.

## Deviations from the handoff, and why

1. **Wire spelling `legacy_pending`, not `agentsroom_import_pending`.** The
   checked-in contract guard (`tests/contract/mcp_mutants.rs`) forbids
   model-facing tool vocabulary from naming the legacy tracker, and `import` also
   trips its `port` needle. The concept is unchanged; the value names Kontor's own
   state and the legacy system is named by the import manifest's `source`.
   Renaming was preferred to adding an exemption to a deliberate guard.
2. **`Repository::create_project` defaults to native origins** instead of gaining
   a required field at 39 call sites. It carries no provenance and cannot honestly
   claim any other origin; declaring a legacy origin is `projects:ensure`'s job.
3. **Memory import no longer requires a freeze before it runs.** The handoff's own
   ordering puts the attestation at step 4 and the switch at step 5, so requiring
   a frozen source at import time would reinstate the ordering that made the
   global ceremony necessary.
4. **v21's `memory_import_manifests` is still consulted by preview** (writes go to
   `subject_import_manifests` only), so a database that imported under the old
   table is not told the same export is pending and does not import it twice.
5. **Receipts cover `import`, `attest`, `switch` only.** A preview writes nothing,
   so it earns no receipt and the closed operation list has no unreachable value.

## Corrected by REMEDIATION-01

The atomicity this document originally claimed was not delivered: the import
committed its items and its manifest in two transactions, so a failure between
them left items behind with no manifest, and the retry — which derives
`already_imported` from the manifest — re-ran the item loop and died on their
primary key. The reviewer found it; `REMEDIATION-01.md` records the fix and the
test that now fails without it.

## Not started

Checkpoints 2–6: the native `kontor-jira` crate and its configuration/keychain,
Jira materialization and ASMA Epic activation, the reconcile/comment
implementations, backlog import/cutover, and the ASMA CLI refusals plus the
promotion-context import. Backlog writes are **not** yet gated by
`(project, backlog)` authority — that gating is checkpoint 5, and the ledger row
it will read already exists.
