Artifact: high-scope-record

# ASMA-8116 high-scope record

## Settlement

This record settles the scope turn for ASMA-8116. It is the implementation
contract and handoff, not implementation-completion evidence. The current
working-tree changes are provisional, uncommitted, and incomplete against the
immutable Jira issue-identity requirement below.

The bounded subject is task `01a07722-c34b-74f3-9880-c3361ff88021` under epic
`01a0539a-51c9-7301-9bd7-26c09167b23e` and project
`01a0064a-e056-7603-9968-ef64fdaacb75`. No other Jira-key task is admitted by
this record.

## Resolver and public-projection contract

1. Jira issue identity is two-part: the current canonical Jira key is mutable,
   while the Jira REST top-level `id` is immutable. Connector readback must
   retain both values and its existing readback evidence.
2. Both `jira_epic_bindings` and `jira_task_binding_confirmations` must retain
   the immutable Jira issue ID. Migration must preserve existing bindings and
   must never synthesize an ID.
3. A confirmed key change is accepted only when fresh Jira readback has the
   same immutable issue ID as the confirmation ledger. The ledger is updated
   transactionally while the Kontor epic/task UUID remains unchanged.
4. A readback for a different immutable issue ID is a typed anti-rebind
   refusal. A legacy migrated row without an immutable ID remains fail-closed
   for key changes until supported Jira readback establishes the ID.
5. Resolution is project-scoped and accepts only an exact canonical Jira key.
   It searches confirmed epic and confirmed task bindings and returns the
   immutable Kontor subject UUID, subject kind, current key, durable readback
   evidence, confirmation time, and binding revision. Draft or merely planned
   links do not resolve as confirmed bindings.
6. Malformed, foreign-project, missing, unconfirmed, anti-rebind, duplicate,
   and ambiguous inputs return stable typed refusals. A uniqueness violation or
   ambiguous read must never escape as a raw `rusqlite` backend error, including
   concurrent confirmation and key-change races.
7. One confirmed Jira issue may identify only one subject in the project,
   including across the epic and task ledgers. Database constraints remain the
   final concurrency boundary; application checks exist to select the typed
   domain error.
8. Public task readback and epic/task backlog projections expose a structured
   Jira-binding state. Awaiting bindings expose `awaiting`; confirmed bindings
   expose the exact key, readback hash, confirmation time, and binding revision.
   The existing Kontor UUID stays the stable public identity across confirmation
   and same-issue Jira key changes.
9. Omitting a legacy epic backlog code does not allocate a new legacy namespace.
   Existing explicit legacy codes remain readable and usable only on their
   compatibility paths; a legacy code is never inferred from a Jira key and is
   never the authority used by the resolver.
10. OQ-001 is resolved by clauses 1-4. The earlier scope reduction that refused
    every confirmed key change is not the admitted implementation.

## Provisional changes made during scope

The uncommitted working tree currently contains these exploratory changes:

- `kontor-store` defines confirmed Jira binding subject/state projections, an
  exact project-scoped key resolver, canonical-key validation, and an
  application-level cross-ledger collision guard.
- `kontor-store` makes applied epic backlog code optional and stops allocating a
  legacy code when apply omitted one, while preserving explicit legacy codes.
- `kontor-api` and `kontor-daemon` add structured Jira binding data to task
  snapshots and epic/task backlog projections; OpenAPI and the generated console
  schema were regenerated.
- Store and daemon regressions cover awaiting and confirmed projections, exact
  resolution, malformed/foreign/missing/unconfirmed keys, UUID and evidence
  preservation, collision refusal, optional legacy codes, and replay.
- `BINDING-VALIDATION-MUTANT.md` records the killed cross-subject validation
  mutant.

These changes do not yet retain Jira's immutable top-level issue ID, do not
implement safe same-issue key changes, and do not establish typed mapping for
every SQLite uniqueness/race path. They must be reviewed and adapted after
branch integration; their presence is not authority to preserve their current
shape.

## Verification performed during scope

| Command or exact check | Outcome |
| --- | --- |
| `cargo check -p kontor-store -p kontor-api -p kontor-daemon` | PASS |
| `cargo test -p kontor-store --test jira_materialization` | PASS, 10/10 |
| `cargo test -p kontor-store` | PASS; all unit, integration, and doc-test targets |
| `cargo test -p kontor-api` | PASS: library 23/23, error envelope 5/5, OpenAPI contract 3/3 |
| `KONTOR_UPDATE_CONTRACT=1 cargo test -p kontor-api --test openapi_contract` | PASS, 3/3; contract regenerated |
| `pnpm --filter kontor-console generate:api` | PASS |
| `pnpm --filter kontor-console verify:api` | PASS |
| `pnpm --filter kontor-console typecheck` | PASS after adding the required awaiting fixture projection |
| `pnpm --filter kontor-console test` | PASS, 16 files and 300 tests |
| `cargo clippy -p kontor-store -p kontor-api -p kontor-daemon --all-targets -- -D warnings` | PASS |
| `cargo test -p kontor-daemon --test loopback_api a_task_snapshot_reports_the_pinned_specification_revisions -- --exact` | PASS, 1/1 |
| `cargo test -p kontor-daemon --test loopback_api epic_backlog_code_previews_applies_and_reads_back_independently_of_execution_scope -- --exact` | PASS, 1/1 |
| `cargo test -p kontor-daemon --test loopback_api an_applied_task_materializes_and_replays_without_a_startup_task_scope -- --exact` | PASS, 1/1 after correcting the provisional fixture to use distinct epic/task Jira keys |
| Unrestricted `cargo test -p kontor-daemon --test loopback_api automatic_jira_reconciliation_records_and_resolves_an_unfinished_held_epic_without_effects -- --exact` | PASS, 1/1 |
| Unrestricted `cargo test -p kontor-daemon --test loopback_api` | PASS: 302 passed, 0 failed, 1 ignored, finished in 1024.66s |

The first sandboxed full loopback attempt exposed a real invalid fixture that
assigned one Jira key to both an epic and a task; the fixture was corrected and
its exact regression passed. A later sandboxed attempt reached a Wiremock port
permission denial in the automatic Jira reconciliation case. The unrestricted
exact check and the final unrestricted full suite both passed that case.

For mutation evidence, the shared epic/task collision guard was temporarily
disabled. The exact `activation_requires_every_confirmed_binding_and_survives_readback`
test then failed at its required typed-conflict assertion. Restoring the guard
returned the same exact test to PASS. No mutation was retained.

All test databases and connectors used by these checks were isolated. The live
Kontor/Paseo state, active TeamRun, topology, and OpenCode posture were not
changed. The untracked `.pnpm-store/` created during schema work was removed and
is absent at settlement.

## Remaining implementation handoff

1. Fetch and integrate the deployed `origin/master` before implementation. The
   current branch is `0422361298de97e51ed3202e5b6e506cf94a3bab`; the locally
   observed `origin/master` is `40ad3a54bce07b78d29270456f64b3ed72083f5c`
   and the histories are 2/3 commits divergent. Confirm the integrated upstream
   contains deployed PR205 / merge
   `177146015e186585d61e11c0ba1de7af92e4ee8c`.
2. Add the schema migration and connector/store/API threading for Jira REST
   top-level issue ID in both confirmation ledgers. Preserve existing rows and
   fail closed rather than fabricating legacy IDs.
3. Implement transactional same-issue key reconciliation and typed different-ID
   anti-rebind refusal while preserving the existing Kontor subject UUID.
4. Map duplicate and ambiguous constraints, including races and migrated corrupt
   states, to stable typed domain conflicts instead of raw `rusqlite` errors.
5. Complete focused migration, export, backup, recovery, same-issue key-change,
   different-issue anti-rebind, duplicate, ambiguity, and concurrent-race tests.
   Cover both epic and task ledgers and restore/reopen behavior.
6. Reconcile the provisional resolver and public projections with the integrated
   branch, regenerate checked-in contracts, and rerun the exact checks above plus
   the new migration and recovery coverage.
7. Preserve the active team. The scheduler projection gap (`started=[]` while
   its TeamRun is running) and live Paseo's missing `providerOptionsApplied`
   remain operational gaps. Do not weaken the frozen high-stakes OpenCode proof,
   replace topology, or silently widen Jira scope; any required runtime
   correction goes to the existing LSA/integration owner through Kontor.

No commit, merge, rebase, runtime correction, or implementation-phase work was
performed as part of this settlement.
