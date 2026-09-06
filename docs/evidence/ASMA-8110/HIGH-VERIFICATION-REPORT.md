# ASMA-8110 high-verification report: second-remediation verification

Date: 2026-09-07
Artifact: `high-verification-report`
Task: `ASMA-8110` / `01a07391-328e-74a3-a808-e7b5775c8438`
Phase: `high-verification`
TeamRun: `01a07398-b8d2-7363-8dcc-e92c061deffa`
Candidate: `438f9760eb9faa1b5a830b751a476c3c7a24e44d`
Handoff evidence hash: `a71447d477be81cdc0264d191bb06275aa50514e3bc66888bd7b0e984a23b935`
Status: **rejected — the authoritative archive gate is red and the handed evidence is not candidate-consistent**

## Verdict

Candidate `438f976` repairs the three code/test defects proved in the preceding
verification. The exact archived tree rejects a present `result: null`, keeps
invalid source-receipt refusals from disclosing an unrelated route receipt, and
proves the both-prechecks-empty recovery interleaving at store level. Its named
route, replay, fencing, migration, and restart cases passed.

It does not satisfy the full contract. The authoritative online archive command
exited 1 when the full-load schema suite produced `DatabaseBusy` in
`a_concurrent_first_open_initializes_exactly_one_realm`. Five immediate isolated
reruns passed, establishing that the failure is load-sensitive, but the scope
requires the authoritative command itself to exit 0 and supplies no exception
for a flaky baseline.

The exact candidate also contains two handoff-document defects. Its merged code
is schema v92, while its scope still asserts v90 and requires a v90 delivery
readback. Its high-change record still ends by freezing rejected candidate
`5743d7a` and reports that older candidate's partial archive run rather than the
handed `438f976` tree.

No production source was changed during verification. No daemon was started or
stopped, no deployment or recovery was invoked, and no task, workflow, Jira,
runtime, topology, TeamRun, AgentRun, seat, or native-session state was mutated.

## Candidate and handoff boundary

- Implement turn ordinal 4 is durably settled with artifacts
  `focused-newest-master-validation`, `high-change`, `operational_gap`, and
  `repair-head-438f9760eb9faa1b5a830b751a476c3c7a24e44d`.
- Its evidence hash is
  `a71447d477be81cdc0264d191bb06275aa50514e3bc66888bd7b0e984a23b935`;
  its terminal timeline position is `6:3169`.
- A scratch repository created from `git archive 438f976` had tree hash
  `72d45ec8f700cd1718feb71c6587ca5b67411862`, exactly equal to
  `438f976^{tree}`. All candidate results below ran from that tree.
- The scratch tree was still clean and retained the same tree hash after the
  test run.
- During verification the shared branch advanced through `4fd5b18`, `d49b151`,
  `f01bfcf`, `069d321`, and `b0b5e8e`. None belongs to the settled implement
  turn, so none was substituted for the exact candidate or evaluated here.
- The shared worktree was externally removed during the first reporting attempt
  and later restored at the same registered path. Verification continued from
  the already-proved exact archive; QA did not recreate, replace, or register a
  workspace.

The preceding rejection of `5743d7a` remains preserved in Git at `25265c8`.
This report supersedes its working-tree contents while retaining that immutable
history.

## Findings

### F-8110-R6 — high: the authoritative archive gate exits nonzero

`python3 scripts/verify-tree.py --mode archive` ran with registry access from
the scratch repository whose committed tree exactly equals `438f976^{tree}`.
It established:

```text
Cargo.lock byte-compare: identical
cargo fmt --all -- --check                         passed
cargo clippy --workspace --all-targets -D warnings passed
cargo test --workspace --locked                    failed
```

The workspace run reached `crates/kontor-store/tests/schema_v1.rs` after all
earlier executed suites were green. That binary ended:

```text
failures:
    a_concurrent_first_open_initializes_exactly_one_realm

SqliteFailure(Error { code: DatabaseBusy, extended_code: 5 },
              Some("database is locked"))

test result: FAILED. 56 passed; 1 failed
```

Five immediate exact isolated reruns of that test each passed in 3.5–4 seconds.
The implement activity had independently observed the same full-load failure and
three isolated passes. This classifies the symptom as load-sensitive rather
than an ASMA-8110 behavioral regression, but it does not turn the required
archive exit into a pass.

The failure stopped `cargo test --workspace --locked`; therefore `cargo audit`,
`cargo deny check`, frozen pnpm install, typecheck, Vitest, and the production
dependency audit were not reached.

Required correction: isolate or fix the full-load database-lock failure, or
obtain an explicit scope disposition that changes the gate contract, then run
the complete authoritative archive command successfully on a newly handed exact
SHA.

### F-8110-R7 — medium: the candidate's delivery generation is still stale

The exact candidate merges current master `508a514`, which includes migrations
0091 and 0092. Its executable schema constant is therefore 92, and the read-only
realm census also observed `PRAGMA user_version = 92`.

The exact candidate's `HIGH-SCOPE-RECORD.md` nevertheless says:

- the candidate `SCHEMA_VERSION` is 90;
- the deployed realm reads back at 90; and
- delivery must require schema version 90 and migration 0090 exactly once.

Migration 0090 remains the ASMA-8110 route migration, but it is no longer the
candidate's terminal schema generation. A binary built from this candidate
upgrades through 0092, so the protected delivery check cannot succeed as
written.

Commit `4fd5b18`, created after the handoff, appears to correct this document.
It is follow-on evidence only until an implement turn durably hands it over.

Required correction: hand over a candidate whose frozen scope names terminal
schema 92 while continuing to identify 0090 as the route migration.

### F-8110-R8 — medium: the handed high-change record freezes another SHA

The exact candidate's high-change record describes the new null-binding,
source-refusal, concurrency, and lockfile work at its top, but its verification
and handoff sections still say:

```text
python3 scripts/verify-tree.py --mode archive on 5743d7a
Freeze 5743d7a. It is the remediation candidate.
```

That conflicts with the durable role-turn artifact, which freezes `438f976`.
The ledger was sufficiently specific to remove ambiguity for this verification,
but the owed `high-change` document is not a self-consistent handoff record and
does not report this candidate's archive result.

Commit `d49b151`, created after the handoff, appears to add a final-candidate
section. It was not part of implement turn 4 and is not accepted by this report.

Required correction: hand over a high-change record that names the exact new
candidate, its base/integration boundary, and the archive result actually run
for that SHA.

## Closed findings and verified behavior

Static review and the exact candidate's tests close the preceding code findings:

- F-8110-R1's lock drift is closed at the verification instant: online
  regeneration was byte-identical before compilation began.
- F-8110-R2 is closed: only an absent `result` member takes the legacy path;
  present null and other present malformed bindings parse strictly and refuse.
- F-8110-R3 is closed: post-transaction decoration is gated on
  `RepositoryError::Conflict { subject: "gate rejection route", .. }`; other
  source-identity errors retain their typed refusal and disclose no route
  receipt.
- F-8110-R4 is closed by the combined tests: the store case proves both route
  pre-checks are empty before either transaction opens, the same-source loser is
  stopped by workflow CAS, and the separate current-state identity collision
  exercises the route-uniqueness conflict that the service decorates.
- The route retains immutable route-time `team_run_id`; later TeamRuns cannot
  release its fence.
- A released rejection returns to verification and remains there until a fresh
  verdict; reconciliation and an ordinary reviewer turn do not invent one.
- Historical recovery remains source-unique, atomic with receipt and workflow
  movement, replay-stable, append-only, and durable across reopen.

These passing behaviors do not cure the red authoritative gate or the handed
evidence inconsistencies.

## Test record for exact candidate `438f976`

| Command or observation | Result |
|---|---|
| Exact exported tree hash | `72d45ec8f700cd1718feb71c6587ca5b67411862`, equal to `438f976^{tree}` |
| `python3 scripts/verify-tree.py --mode archive` | **failed**, exit 1 in workspace tests |
| Online `cargo generate-lockfile` byte comparison | identical |
| `cargo fmt --all -- --check` | passed inside archive gate |
| `cargo clippy --workspace --all-targets -- -D warnings` | passed inside archive gate |
| daemon `loopback_api` within workspace | 301 passed, 0 failed, 1 explicitly ignored |
| `a_post_route_invalid_source_keeps_its_refusal_and_discloses_no_route_receipt` | passed |
| `gate_rejection_recovery_refuses_invalid_exact_bindings_but_accepts_an_absent_legacy_binding` | passed |
| `concurrent_fresh_recovery_keys_name_the_single_original_route_receipt` | passed |
| `a_released_rejection_stays_in_verification_until_a_fresh_gate_verdict` | passed |
| store `repository_roundtrip` within workspace | 79 passed, including deterministic pre-check, atomicity, uniqueness, and reopen cases |
| store `backup_snapshot` within workspace | 12 passed, including route migration/snapshot |
| store `schema_v1` within workspace | **56 passed, 1 failed** (`DatabaseBusy`) |
| isolated exact first-open test, five repetitions | 5/5 passed |
| supplemental offline MCP vocabulary test | did not execute: its build requested an uncached Swagger UI archive while network was intentionally disabled |

## Read-only realm census

Before the announced control-plane redeploy window, the existing realm was
observed without mutation:

- schema version 92; `PRAGMA integrity_check` returned `ok`; foreign-key check
  returned zero rows;
- one route exists for ASMA-8110's `high-verification-gate` rejection, from
  workflow revision 5 to `high-implementation@6`, bound to this TeamRun;
- ASMA-8110 is `ready` and its active workflow remains
  `high-implementation@6`;
- ASMA-8100 remains `done@5` with its active workflow at `final-review@5`;
- no route consumes protected ASMA-8100 receipt
  `01a07373-0b66-7b93-905f-c2a21bee494f`.

No Kontor call was made during the announced binary-swap window. These are
read-only observations, not delivery qualification.

## Open questions

None. The durable implement turn unambiguously freezes `438f976`; the later
branch commits are unhanded follow-on work and were not assumed to belong to the
candidate. The disappeared worktree was externally restored before this report
was written, so no unresolved workspace-location choice remains.

## Exit criteria for another verification turn

1. Produce a newly settled implement handoff with one exact candidate SHA.
2. Ensure its scope names schema v92 and route migration 0090 without a
   contradictory delivery readback.
3. Ensure its high-change record names that exact SHA and reports tests run on
   that tree.
4. Make the authoritative online archive command complete with exit 0, including
   workspace tests, audit/deny, frozen pnpm install, typecheck, Vitest, and the
   production dependency audit.
5. Re-run high verification from that exact committed tree. Do not deploy,
   invoke recovery against ASMA-8100, or treat its terminal historical receipt
   as consumed.
