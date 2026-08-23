# KON-OP-07 / ASMA-7876 Release Notes

> **Date:** 2026-08-19 10:38
> **Status:** 🟢 Approved
> **Author:** Architect · OP-07
> **Category:** report
> **Scope:** KON-OP-07 project/subject authority cutover at `d3ecbecd53fb1e1bf9076946e2f703f7973c9b83`
> **Summary:** Approves the schema-32 project/subject authority slice for release after independent code review and QA. Native Jira ownership is explicitly outside this release and remains successor work under the epic owner's accepted re-scope.

---

## When to Load

**Load this document when:**
- releasing or auditing ASMA-7876 / KON-OP-07;
- upgrading a current Kontor realm from schema 53 to schema 54;
- changing project bootstrap, memory cutover, or backlog-authority enforcement;
- checking which parts of the original OP-07 Jira scope did not ship here.

**Do NOT load for:** implementing the native Jira connector or evaluating a later backlog-import implementation.

---

## Release Verdict

**PASS for the architect-owned `release-gate` on the delivered, re-scoped
authority slice.**

Kontor readback for task `01a0074f-672c-7f70-8bdd-da707dcda0ce` reports both
preceding gates passed. The reviewed code is submodule commit `d3ecbec`; QA
evidence is committed at `095d468`, and superproject commit `fcafd28` points to
that evidence revision. The evidence-only commit changes no runtime source.

This verdict relies on the epic-owner decision accepted by the independent
inspector: native Jira connector/materialization work is not part of this release.
It must not be inferred from this gate that the original six-checkpoint
architecture handoff shipped in full.

## Released Behavior

- Authority is now recorded per `(project_id, memory | backlog)` rather than in a
  realm-wide singleton. Immutable origin is separate from current write authority.
- A fresh `kontor_native` subject is writable immediately and cannot be forced
  through a synthetic freeze/export/empty-import ceremony.
- A `legacy_pending` memory subject stays legacy-owned until its import manifest,
  source-freeze attestation, stored-state readback hash, and one-way switch all
  exist. Import, attestation, and switch produce immutable receipts.
- Memory items, revisions, approvals, readback, and the import manifest commit in
  one transaction. A manifest failure therefore leaves nothing, and retry begins
  cleanly instead of encountering an unmanifested partial import.
- `projects:ensure` declares both subject origins, includes those origins in its
  idempotency intent, reports current authority, and refuses a replay that states
  different origins.
- `epics:apply` and task lifecycle transitions refuse while Kontor does not own
  the project's backlog. Until backlog import/readback/switch exists,
  `backlog_origin: legacy_pending` is rejected at project creation so an
  unsupported dead-end state cannot be created through a public surface.
- Observer and Admin surfaces expose project authority through
  `GET /v1/projects/{project_id}/subjects/authority` and per-project attestation.
  MCP adds `kontor_subject_authority_get` and
  `kontor_subject_authority_attest` with the same semantic boundary.
- The retired realm-wide freeze route remains present only to return typed
  `400 invalid_request` with the per-project replacement. It cannot mutate the
  frozen singleton.

## Data and Upgrade

The store schema advances from **31 to 32** through
`0054_project_subject_authority.sql`.

- Existing projects seed backlog as native because their graph already lives in
  Kontor.
- Existing projects seed memory from the last realm-wide authority claim.
- Database constraints make a native subject with cutover evidence and a switched
  legacy subject with incomplete evidence unrepresentable.
- Triggers permit only one source-freeze attestation and one forward authority
  switch; origin changes, deletion, reversal, and receipt/manifest mutation fail.
- The old `memory_authority` row is retained as historical evidence and frozen
  against further updates.
- Preview still recognizes a legacy v21 memory-import manifest, preventing a realm
  upgraded after an earlier import from importing the same export twice.

Upgrade is automatic when the store opens. Downgrade is unsupported, consistent
with the existing migration contract.

## Release Evidence

The independent inspector re-ran the complete repository gates at `d3ecbec`:

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --workspace --no-fail-fast` | pass — 1401 tests |
| `cargo deny check` | pass |
| `pnpm --filter kontor-console verify:api` | pass |
| `pnpm -r typecheck` | pass |
| `pnpm -r test` | pass — 278 tests |

QA independently re-ran the three remediation regressions at the same code
revision: manifest-failure rollback/resume, legacy-owned backlog refusal, and
fresh-native project bootstrap. All passed. The inspector also mutation-checked
the manifest transaction and epic-apply backlog guard; both tests failed under
their corresponding defect.

The release-seat rerun on 2026-08-19 also passed: both schema-32 migration tests
(2/2), manifest-failure rollback/resume (1/1), legacy-owned backlog refusal
(1/1), and fresh-native project bootstrap (1/1). The daemon tests initially
stopped during dependency setup because the sandbox could not resolve the pinned
Swagger UI download host; rerunning the same commands with network access fetched
that build asset and both tests passed without a source change.

## Deliberate Limitations and Follow-Up

- **Native Jira ownership did not ship.** `kontor-jira`, connector configuration,
  Jira materialization/activation, native reconcile/comment behavior, and ASMA CLI
  Jira refusals/forwarders are successor scope. The existing
  `TicketDelegation`/`AsmaExecutable` path therefore remains and must not be
  described as replaced by this release.
- **Legacy backlog import did not ship.** The domain can represent a pending
  backlog for migration tests and future work, but the public project-bootstrap
  surface refuses that origin until import, readback, and switch exist.
- **One proof gap is accepted as non-blocking.** The lifecycle-transition backlog
  guard is present, but no regression currently calls that seam under withheld
  authority. Add it before a public surface can create a pending backlog. The same
  future change must audit all other graph/intake/policy write seams.
- FTS rebuild remains outside the memory-import transaction because it is a
  derived, rebuildable projection. A rebuild failure does not roll back the
  authoritative imported rows or manifest.

## Evidence References

- `docs/evidence/KON-OP-07/CHECKPOINT-01.md`
- `docs/evidence/KON-OP-07/REMEDIATION-01.md`
- `docs/evidence/KON-OP-07/CODE-REVIEW-01.md`
- `docs/evidence/KON-OP-07/QA-01.md`
