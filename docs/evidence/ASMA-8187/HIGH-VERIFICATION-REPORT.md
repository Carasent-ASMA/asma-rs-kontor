# ASMA-8187 high-verification report

Date: 2026-09-16
Artifact: `high-verification-report`
Task: Jira `ASMA-8187` / Kontor `01a0a943-652b-79d0-9f0c-f4965a33d7ab`
Phase: `high-verification`
Verification sequence: 2
Candidate: `99ad64c9cc7a5684900f2c21d8dfb728ac4697ec`
Exact base: `4077a590aa9a4f90fb82147f14af1699b833a810`
Prior rejected candidate: `1fcd42d61b2a93ad6ee1f344d0f82f634dbc2381`
Scope: [`HIGH-SCOPE-RECORD.md`](HIGH-SCOPE-RECORD.md)
Implementation evidence: [`HIGH-CHANGE-RECORD.md`](HIGH-CHANGE-RECORD.md)
Status: **FAIL — the pushed remediation candidate still does not satisfy the recorded high-scope contract**

## Verdict

The exact pushed candidate is rejected for high verification.

The remediation closes the previously demonstrated store-commit recovery gap:
the new succession row commits with the hosted-seat replacement, and a targeted
mutant that removes that row from the transaction is killed by the new
store-loss recovery test. The focused stale-native suites, lost archive and
launch acknowledgements, generation/history preservation, remediation-generation
authority, touched-crate lint, MCP tests, and generated OpenAPI/client parity
also pass.

Four blocking findings remain:

1. the candidate adds schema v97 but leaves the schema contract at v96 and omits
   the new table from the exact expected-table set, producing two
   candidate-specific checked-in test failures;
2. preview returns a hash and selected summaries, not the scope-required
   deterministic intent document containing every fenced value;
3. durable readback does not contain the complete required ECP placement, and
   its named replay test accepts a deliberately wrong container native id;
4. the named approved-route-authority test remains green when account authority
   is removed from the approved-route digest, because the same setup also trips
   the independent headroom-observation fence.

No production code was changed. Every mutation was made in an isolated archive
of the candidate, one at a time, then restored byte-for-byte. Generated
contracts reproduced without drift. The only retained worktree change from this
turn is this evidence report.

## Candidate identity and boundary

The module worktree was clean at handoff and resolved as follows:

```text
$ git rev-parse HEAD
99ad64c9cc7a5684900f2c21d8dfb728ac4697ec

$ git rev-parse HEAD^
4077a590aa9a4f90fb82147f14af1699b833a810

$ git ls-remote --heads origin refs/heads/feat/ASMA-8187-repair-admin-preview-apply-stale-native-core-team-succession
99ad64c9cc7a5684900f2c21d8dfb728ac4697ec refs/heads/feat/ASMA-8187-repair-admin-preview-apply-stale-native-core-team-succession
```

`git diff --stat HEAD^..HEAD` reports 15 files, 2,033 insertions and 65
deletions. Review covered that complete delta: schema migration and store,
API/OpenAPI/client, daemon flow, runtime-facing tests, and the change record.

The codebase knowledge graph was consulted before source exploration and then
reindexed at this candidate. Its candidate-vs-base scan reported 15 changed
files, 110 changed seed symbols and 637 impacted symbols. Its metadata matched
the changed Rust sources. The new SQL migration was parse-partial, so it was
read directly in full rather than inferred from graph coverage.

## Blocking findings

### F-8187-V5 — high: schema v97 fails its checked-in schema contracts

Migration `0097_core_team_route_succession_recovery.sql` and
`SCHEMA_VERSION = 97` are present, but
`crates/kontor-store/tests/schema_v1.rs:553` still asserts version 96. The same
test file's exact expected-table set does not include
`core_team_route_successions`, so the shape assertion at line 2737 also fails.

Observed in the broad touched-crate command:

```text
an_empty_database_migrates_to_the_current_schema_version
FAIL: left 97, right 96

the_schema_contains_exactly_the_expected_tables_and_they_are_all_strict
FAIL: actual set contains core_team_route_successions; expected set does not

schema_v1 result: 56 passed, 2 failed; command exit 101
```

Both exact tests pass at parent `4077a590...`; these are candidate-specific
regressions, not inherited failures. A candidate that changes persistent schema
while its schema/version contract suite is red cannot pass high verification.

Required correction: update the version contract and exact table inventory for
v97, add the migration-specific preservation assertions appropriate to this
immutable recovery ledger, and rerun the full touched-crate command green.

### F-8187-V6 — high: preview does not return the complete deterministic intent document

The scope requires preview to return one deterministic intent document and its
hash, covering all named logical binding, occupancy, predecessor, ECP placement,
route, Team Definition, topology/Core Team, completion, remediation and headroom
evidence.

Internally, `core_team_route_plan` builds such a canonical JSON value at
`crates/kontor-daemon/src/applications.rs:7084-7162`. Public
`CoreTeamRoutePreviewDto`, however, exposes only selected summary fields at
`crates/kontor-api/src/applications.rs:1063-1112`: it has no intent-document
field and does not otherwise expose the SeatBinding revision, occupancy
generation, full predecessor identity, full ECP placement, topology/Core Team
pins, completion pin or remediation-generation evidence.

The hash can detect server-side drift, but a caller cannot read, retain or audit
the complete evidence the hash attests. Returning an approved-route digest and
Team Definition summary closes part of prior F-8187-V2; it does not fulfill the
scope's complete preview-evidence contract.

Required correction: return the canonical, versioned intent document (or an
equally complete typed representation of that exact document) beside its hash,
regenerate contracts, and assert every required field against authoritative
fixture values.

### F-8187-V7 — high: durable readback omits required placement identity and its test accepts wrong placement

The scope defines exact ECP placement as logical node id, native project id,
native workspace id, canonical cwd, container generation and provider
conversation/session correlation. `CoreTeamRoutePlacementDto` at
`crates/kontor-api/src/applications.rs:1017-1032` contains only topology node id,
container binding id, one generic container native id and cwd. It has no native
project id and no container generation. The occupant provider-session fields do
not supply the missing container identities.

The durable readback constructor at
`crates/kontor-daemon/src/applications.rs:7352-7365` consequently cannot persist
those missing values. The named replay test at
`crates/kontor-daemon/tests/loopback_api.rs:47593-47618` checks only that
`placement.container_native_id` and cwd are strings; it never compares the
container id to the fixture's actual ECP workspace.

Independent mutation confirmed the gap. Replacing the readback's
`plan.container.identity.native_id` with the predecessor seat native id left
`an_exact_replay_after_the_receipt_landed_answers_from_durable_evidence` green.
Thus the test proves stable replay of stored bytes, but not exact placement.

Required correction: persist and return all scope-defined placement identities
and generation/correlation evidence, then assert exact expected values so a
wrong project, workspace, generation or correlation is rejected by the test.

### F-8187-V8 — high: the approved-route authority test is confounded and its targeted mutant survives

The implementation includes governed account authority in the approved-route
digest at `crates/kontor-daemon/src/applications.rs:6985-6992`. The named test
`a_core_team_succession_refuses_an_approved_route_authority_that_moved`, however,
adds a second provider observation after preview. That also changes the
headroom-account state independently of the approved-route digest.

In an isolated candidate archive, replacing the server-derived
`account_authority` value with a fixed constant left the named test green. The
test's refusal therefore does not establish that the approved-route authority
digest is doing the refusing; the independent headroom fence can satisfy the
same assertion. This leaves the scope-required one-by-one approved-route/digest
mutation case unproved.

Required correction: isolate authority drift from headroom-observation drift,
or add a direct focused case whose only changed preview input is the governed
account authority/digest, and show that removing that digest input makes the
test fail with an otherwise effect-free apply.

## Scope behavior exercised successfully

The following candidate behavior was independently exercised successfully:

- stale-native succession and its focused Core Team cases: 18/18;
- succession/replay/lost-ack cases: 11/11;
- generation-1 history remains append-only and one generation-2 active
  occupant is installed on the same logical SeatBinding;
- immediate provider headroom is rechecked and a lapsed-capacity mutant is
  killed;
- SeatBinding/preview CAS and remediation-generation mutants are killed;
- removing the new atomic succession-ledger write is killed by the injected
  post-store-commit loss case;
- exact receipt replay returns stored evidence without runtime calls or seat
  mutation in the covered case;
- operational-liveness, MCP, formatting, and touched-crate clippy checks pass;
- OpenAPI and generated TypeScript client artifacts reproduce byte-for-byte.

These passes do not waive the four blocking findings above.

## Exact command and result record

### Focused stale-native, replay, history and authority suites

```text
$ cargo test -p kontor-daemon --test loopback_api core_team -- --nocapture
PASS: 18 passed, 0 failed

$ cargo test -p kontor-daemon --test loopback_api succession -- --nocapture
PASS: 11 passed, 0 failed

$ cargo test -p kontor-store --test operational_liveness
PASS: 11 passed, 0 failed
```

The focused sets include the exhaustive named drift table, lost archive and
launch acknowledgements, injected loss after store replacement, replay after
receipt persistence, exact-key replay/conflict behavior, generation/history
preservation, and generation-2 remediation authority.

### Touched crates and generated contract parity

```text
$ cargo test -p kontor-api -p kontor-core -p kontor-runtime -p kontor-runtime-paseo -p kontor-store
FAIL: exit 101; all reached API/core/runtime/Paseo groups passed, including
      209 kontor-runtime-paseo contract tests with 6 live tests ignored;
      kontor-store schema_v1 failed 2 of 58 tests (F-8187-V5)

$ cargo test -p kontor-mcp
PASS: 65 passed, 0 failed (57 library, 3 binary, 5 seat-contract tests)

$ KONTOR_UPDATE_CONTRACT=1 cargo test -p kontor-api --test openapi_contract
PASS: 3 passed, 0 failed

$ pnpm --filter kontor-console generate:api
PASS: openapi-typescript 7.13.0 generated apps/console/src/api/schema.d.ts

$ git diff --exit-code -- crates/kontor-api/contract/openapi.json apps/console/src/api/schema.d.ts
PASS: exit 0, no generated drift
```

### Formatting, lint, full daemon and archive verification

```text
$ cargo fmt --all -- --check
PASS: exit 0

$ cargo clippy -p kontor-api -p kontor-core -p kontor-daemon -p kontor-runtime -p kontor-runtime-paseo -p kontor-store --all-targets -- -D warnings
PASS: exit 0

$ cargo clippy --workspace --all-targets -- -D warnings
FAIL: error[E0063], missing field `observed_identity` in initializer of
      `kontor_jira::jira::JiraResponse` at tests/e2e/pilot_sections/domain.rs:2468

$ cargo test -p kontor-daemon
RESULT: daemon library 81 passed; daemon binary 2 passed; account_pinning 5 passed;
        loopback_api 335 passed, 7 failed, 1 ignored; exit 101

$ python3 scripts/verify-tree.py --mode archive
FAIL: Cargo.lock regeneration differs byte-for-byte from the committed lockfile
```

The initial full-daemon run was denied only by sandbox loopback binding. The
permitted rerun above is the recorded result. The archive verifier initially
needed permitted network access; its rerun produced the lockfile result above.

## Exact inherited-baseline reproduction

An isolated archive of exact parent
`4077a590aa9a4f90fb82147f14af1699b833a810` was used. The candidate worktree was
not rewritten.

The seven full-daemon failures were:

```text
a_body_overtaken_during_a_lost_confirmation_is_refused
a_human_authored_body_is_preserved_until_replacement_is_authorized
a_publication_that_never_landed_is_still_refused
a_publication_whose_confirmation_was_lost_is_settled_by_refetch
an_epic_placeholder_body_is_typed_reported_and_repairable
reconcile_plan_refuses_to_call_a_placeholder_body_converged
replaying_a_partial_admission_delivers_its_durable_follow_up
```

Each was rerun exactly at the parent:

```text
$ cargo test -p kontor-daemon --test loopback_api a_body_overtaken_during_a_lost_confirmation_is_refused -- --exact
$ cargo test -p kontor-daemon --test loopback_api a_human_authored_body_is_preserved_until_replacement_is_authorized -- --exact
$ cargo test -p kontor-daemon --test loopback_api a_publication_that_never_landed_is_still_refused -- --exact
$ cargo test -p kontor-daemon --test loopback_api a_publication_whose_confirmation_was_lost_is_settled_by_refetch -- --exact
$ cargo test -p kontor-daemon --test loopback_api an_epic_placeholder_body_is_typed_reported_and_repairable -- --exact
$ cargo test -p kontor-daemon --test loopback_api reconcile_plan_refuses_to_call_a_placeholder_body_converged -- --exact
$ cargo test -p kontor-daemon --test loopback_api replaying_a_partial_admission_delivers_its_durable_follow_up -- --exact
BASE REPRODUCED: the first six returned HTTP 503 `unavailable` where 200 was
required; the last returned HTTP 409 `revision_conflict` where 200 was required,
matching the candidate failure classes and assertions.
```

The parent also reproduces the workspace-clippy error with the same missing
field and source line:

```text
$ cargo clippy --workspace --all-targets -- -D warnings
BASE REPRODUCED: identical error[E0063] at tests/e2e/pilot_sections/domain.rs:2468
```

The archive lockfile drift is inherited as well:

```text
# in the exact-parent archive
$ cp Cargo.lock Cargo.lock.committed
$ cargo generate-lockfile
$ cmp Cargo.lock.committed Cargo.lock
BASE REPRODUCED: files differ at char 12736, line 519
```

These failures are classified as inherited and were not masked or credited as
candidate passes.

The two schema failures are not inherited:

```text
# in the exact-parent archive
$ cargo test -p kontor-store --test schema_v1 an_empty_database_migrates_to_the_current_schema_version -- --exact
PASS: 1 passed, 0 failed

$ cargo test -p kontor-store --test schema_v1 the_schema_contains_exactly_the_expected_tables_and_they_are_all_strict -- --exact
PASS: 1 passed, 0 failed
```

## Independent mutation verification

Each mutant was seeded alone in an isolated archive of exact candidate
`99ad64c...`. No mutation touched the shared worktree. After every run the
source was restored; final `cmp` against the candidate's
`crates/kontor-daemon/src/applications.rs` passed byte-for-byte.

| Mutant | Exact focused command | Result |
| --- | --- | --- |
| Remove apply preview-hash/CAS comparison | `cargo test -p kontor-daemon --test loopback_api a_core_team_succession_refuses_a_preview_whose_seat_revision_moved -- --exact` | **Killed:** apply returned 200 where 400 was required |
| Remove immediate stale-native headroom preflight | `cargo test -p kontor-daemon --test loopback_api stale_core_team_succession_refuses_when_capacity_lapsed_after_its_preview -- --exact` | **Killed:** apply returned 200 where 409 was required |
| Remove remediation current-occupancy-generation equality check | `cargo test -p kontor-daemon --test loopback_api advance_and_remediate_judge_the_key_before_the_revision -- --exact` | **Killed:** generation-1 authority returned 200 where 409 was required |
| Omit `Some(&succession)` from the atomic store replacement | `cargo test -p kontor-daemon --test loopback_api a_succession_lost_after_its_store_commit_converges_on_one_receipt -- --exact` | **Killed:** test failed at `the committed succession recorded itself` |
| Replace server-derived approved-route account authority with a fixed constant | `cargo test -p kontor-daemon --test loopback_api a_core_team_succession_refuses_an_approved_route_authority_that_moved -- --exact` | **SURVIVED:** 1 passed, 0 failed (F-8187-V8) |
| Persist the predecessor seat native id as `placement.container_native_id` | `cargo test -p kontor-daemon --test loopback_api an_exact_replay_after_the_receipt_landed_answers_from_durable_evidence -- --exact` | **SURVIVED:** 1 passed, 0 failed (F-8187-V7) |

Mutation score for these required/high-risk cases: **4/6 killed (66.7%)**. The
two survivors are blocking because they target exact claims made by the change
record and exact acceptance evidence required by the scope.

## Exit criteria for another high-verification turn

1. Make the v97 schema/version and exact-table contract suite green and add
   migration-specific preservation coverage.
2. Return the complete deterministic preview intent document, not only selected
   summaries and its hash.
3. Complete durable ECP placement readback with exact project/workspace,
   container generation and correlation evidence, and assert exact values.
4. Isolate approved-account-authority drift from provider-headroom drift and
   kill the authority-removal mutant.
5. Re-run focused tests, touched crates, MCP, OpenAPI/client regeneration,
   format, touched-crate clippy, full daemon classification and all six mutants
   on one exact new pushed candidate.

No gate was recorded and no workflow state was settled by this verification
turn.
