# ASMA-8187 high-verification report

Date: 2026-09-17
Artifact: `high-verification-report`
Task: Jira `ASMA-8187` / Kontor `01a0a943-652b-79d0-9f0c-f4965a33d7ab`
Phase: `high-verification`
Verification sequence: 3
Handed-off local candidate: `aec5c26469b99aef96ae9c28bcedb6e36a9631f6`
First parent: `249e9a4beca0c74452ad98a2075cc45e237307b5`
Merged upstream parent: `2e4b9985fc1073dd25c22b78349d8f36d6b1d4ec`
Prior rejected candidate: `99ad64c9cc7a5684900f2c21d8dfb728ac4697ec`
Original rejected candidate: `1fcd42d61b2a93ad6ee1f344d0f82f634dbc2381`
Scope: [`HIGH-SCOPE-RECORD.md`](HIGH-SCOPE-RECORD.md)
Implementation evidence: [`HIGH-CHANGE-RECORD.md`](HIGH-CHANGE-RECORD.md)
Status: **FAIL — no exact pushed sequence-3 candidate exists, and the handed-off local tree still fails the recorded high-scope contract**

## Verdict

High verification fails.

The local merge candidate closes the sequence-2 schema-contract failure: schema
generation 99 and the exact expected-table inventory agree, and all 58 schema
tests pass. Focused stale-native succession, lost-ack/replay, generation/history,
CAS, provider-headroom and remediation-authority tests also pass. The complete
non-API touched-crate run, touched-crate clippy and MCP suites pass.

Three scope blockers from sequence 2 remain unchanged:

1. preview still does not return the complete canonical intent document required
   by the scope;
2. durable placement readback still omits exact native-project and container-
   generation evidence, and the named replay test accepts a deliberately wrong
   container native id;
3. the approved-route-authority test remains confounded by a separate headroom
   fence and accepts removal of the server-derived account authority.

The tree is additionally red on committed OpenAPI parity, formatting, full
daemon, workspace clippy and archive lockfile reproduction. Exact-parent runs
classify these as inherited: the first seven daemon failures, workspace clippy
and lock drift reproduce on first parent `249e9a4...`; the newly merged OpenAPI,
formatting and session-key daemon failures reproduce on upstream parent
`2e4b9985...`. They are not presented as ASMA-8187 regressions or hidden as
passes, but a red integrated candidate still cannot receive a pass verdict.

No production source or generated artifact was changed. Mutants and generated
contract/client checks ran only in isolated candidate archives.

## Open-question ledger

- **Subject:** exact candidate identity and publication state for verification
  sequence 3.
- **Attached record:** this report and the implementation handoff for
  `ASMA-8187`.
- **Why ambiguous:** the handed-off module checkout is local merge commit
  `aec5c264...`, while exact remote branch readback remains `249e9a4...` and
  `HIGH-CHANGE-RECORD.md` contains no sequence-3 candidate/remediation entry.
- **Options observed:** implementation publishes and records `aec5c264...`;
  implementation publishes and records a different corrective candidate; or
  sequence-3 exact-pushed verification remains unavailable.
- **Disposition:** unresolved. Results against `aec5c264...` are a complete
  provisional verification of that local tree, not evidence that it is the
  exact pushed candidate. The verification seat will not publish the fourteen
  implementation/upstream commits ahead of the remote branch merely to publish
  its evidence report.

## Candidate identity and boundary

```text
$ git rev-parse HEAD
aec5c26469b99aef96ae9c28bcedb6e36a9631f6

$ git rev-parse HEAD^1
249e9a4beca0c74452ad98a2075cc45e237307b5

$ git rev-parse HEAD^2
2e4b9985fc1073dd25c22b78349d8f36d6b1d4ec

$ git ls-remote --heads origin refs/heads/feat/ASMA-8187-repair-admin-preview-apply-stale-native-core-team-succession
249e9a4beca0c74452ad98a2075cc45e237307b5 refs/heads/feat/ASMA-8187-repair-admin-preview-apply-stale-native-core-team-succession
```

`aec5c264...` is a merge of the sequence-2 evidence commit and upstream master.
Its conflict resolution renumbers the ASMA-8187 succession migration to 99,
sets `SCHEMA_VERSION` to 99 and adds `core_team_route_successions` to the exact
table inventory. The first parent differs from `99ad64c...` only by the prior
evidence-report commit, so its production tree is the sequence-2 candidate.

The codebase knowledge graph was reindexed at the handed-off local candidate
before source review. Candidate-versus-first-parent detection reported 27
changed files, 241 changed seed symbols and 992 impacted symbols. Graph metadata
matched the relevant Rust files. The SQL migration was parse-partial and was
read directly in full.

## Blocking scope findings

### F-8187-V6 — high: preview omits the complete deterministic intent document

The scope requires preview to return the complete deterministic intent document
and its hash. `core_team_route_plan` builds a canonical JSON intent internally,
but public `CoreTeamRoutePreviewDto` at
`crates/kontor-api/src/applications.rs:1064` exposes only selected summaries and
the hash. It does not return the document containing every logical binding,
occupancy, predecessor, ECP placement, route, Team Definition, topology/Core
Team, completion, remediation and headroom fence.

This is unchanged from the first parent. A caller still cannot retain or audit
the complete evidence to which the hash commits.

### F-8187-V7 — high: durable placement readback is incomplete and its named test accepts wrong placement

The scope defines ECP placement as logical node id, native project id, native
workspace id, canonical cwd, container generation and provider correlation.
`CoreTeamRoutePlacementDto` at
`crates/kontor-api/src/applications.rs:1018` contains topology node id,
container binding id, one generic container native id and cwd. It has no native
project id or container generation.

An isolated mutant persisted the predecessor seat native id as
`placement.container_native_id`. The exact named replay test at
`crates/kontor-daemon/tests/loopback_api.rs:49354` remained green. It proves
stable replay of stored bytes, not exact placement identity.

### F-8187-V8 — high: the approved-route-authority test remains confounded

The implementation includes server-derived account authority in the route
document at `crates/kontor-daemon/src/applications.rs:6994`. The named authority
test at `crates/kontor-daemon/tests/loopback_api.rs:49442` changes account state
in a way that also changes independent headroom evidence.

An isolated mutant replaced the server-derived authority with a fixed constant.
The exact named test still passed. Its refusal therefore does not prove that the
approved-route authority digest, rather than the headroom fence, caused the
refusal.

## Closed sequence-2 finding

### F-8187-V5 — closed: schema generation and exact table inventory agree

The merge renumbers the succession migration to
`0099_core_team_route_succession_recovery.sql`, sets `SCHEMA_VERSION = 99` at
`crates/kontor-store/src/migrations.rs:37`, includes the new table in the exact
inventory at `crates/kontor-store/tests/schema_v1.rs:70`, and asserts version 99
at line 563.

```text
$ cargo test -p kontor-store --test schema_v1
PASS: 58 passed, 0 failed
```

## Scope behavior exercised successfully

The local candidate successfully exercised:

- the 18 focused Core Team cases, including stale-native CAS/refusal coverage;
- the 11 succession cases, including lost archive/launch/store acknowledgements
  and exact receipt replay;
- immutable generation-1 history and one generation-2 active successor on the
  same logical SeatBinding in the covered happy path;
- immediate provider-headroom refusal and remediation-generation authority;
- all non-API touched crate suites, including 209 Paseo adapter contract tests
  (six live-daemon tests ignored) and all store integration groups;
- 65 MCP tests and touched-crate clippy.

These green results do not satisfy the three blocking contract findings.

## Exact command and result record

### Focused stale-native, replay and schema suites

```text
$ cargo test -p kontor-daemon --test loopback_api core_team -- --nocapture
PASS: 18 passed, 0 failed

$ cargo test -p kontor-daemon --test loopback_api succession -- --nocapture
PASS: 11 passed, 0 failed

$ cargo test -p kontor-daemon --test loopback_api an_exact_replay_after_the_receipt_landed_answers_from_durable_evidence -- --exact
PASS: 1 passed, 0 failed

$ cargo test -p kontor-store --test schema_v1
PASS: 58 passed, 0 failed
```

### Touched crates, MCP, formatting and lint

```text
$ cargo test -p kontor-api -p kontor-core -p kontor-runtime -p kontor-runtime-paseo -p kontor-store
FAIL: kontor-api openapi_contract: 2 passed, 1 failed;
      committed openapi.json does not match the served contract

$ cargo test -p kontor-core -p kontor-runtime -p kontor-runtime-paseo -p kontor-store
PASS: all unit, integration and doc-test groups passed;
      209 Paseo contract tests passed, 6 live-daemon tests ignored;
      store schema_v1 passed 58/58

$ cargo test -p kontor-mcp
PASS: 65 passed, 0 failed (57 library, 3 binary, 5 seat-contract)

$ cargo fmt --all -- --check
FAIL: rustfmt diff at crates/kontor-teams/tests/team_contract.rs:467

$ cargo clippy -p kontor-api -p kontor-core -p kontor-daemon -p kontor-runtime -p kontor-runtime-paseo -p kontor-store --all-targets -- -D warnings
PASS: exit 0

$ cargo clippy --workspace --all-targets -- -D warnings
FAIL: error[E0063], missing `observed_identity` at
      tests/e2e/pilot_sections/domain.rs:2468
```

### Generated OpenAPI and client parity

The committed candidate fails parity before regeneration. In an isolated
candidate archive:

```text
$ KONTOR_UPDATE_CONTRACT=1 cargo test -p kontor-api --test openapi_contract
PASS: 3 passed, 0 failed; isolated openapi.json changed

$ apps/console/node_modules/.bin/openapi-typescript crates/kontor-api/contract/openapi.json -o apps/console/src/api/schema.d.ts
PASS: openapi-typescript 7.13.0; isolated schema.d.ts changed
```

The regenerated diff adds the merged seat-fill route and its DTOs plus the new
capacity fields. Nothing was copied back to the worktree. Exact upstream parent
`2e4b9985...` fails the same committed OpenAPI contract test (2 passed, 1
failed), while first parent `249e9a4...` passes it (3/3). The mismatch is
inherited from the merged upstream parent, not an ASMA-8187-only contract edit.

### Full daemon and archive verification

```text
$ cargo test -p kontor-daemon
FAIL: library 81/81, binary 2/2 and account_pinning 5/5 passed;
      loopback_api 349 passed, 8 failed, 1 ignored

$ python3 scripts/verify-tree.py --mode archive
FAIL: regenerated Cargo.lock differs byte-for-byte from the committed lockfile
```

Seven daemon failures are the prior baseline: six Jira tests receive HTTP 503
instead of 200, and `replaying_a_partial_admission_delivers_its_durable_follow_up`
receives HTTP 409 instead of 200. The eighth,
`a_session_key_must_be_a_stable_client_message_id`, receives HTTP 200 instead
of 400.

## Exact inherited-baseline reproduction

No inherited failure was masked or counted as a candidate pass.

### First parent `249e9a4...`

An isolated archive of the exact first parent reproduced each of the seven
prior daemon failures with an exact named invocation:

```text
$ cargo test -p kontor-daemon --test loopback_api <name> -- --exact
a_body_overtaken_during_a_lost_confirmation_is_refused                    503 != 200
a_human_authored_body_is_preserved_until_replacement_is_authorized        503 != 200
a_publication_that_never_landed_is_still_refused                          503 != 200
a_publication_whose_confirmation_was_lost_is_settled_by_refetch           503 != 200
an_epic_placeholder_body_is_typed_reported_and_repairable                 503 != 200
reconcile_plan_refuses_to_call_a_placeholder_body_converged               503 != 200
replaying_a_partial_admission_delivers_its_durable_follow_up               409 != 200
```

The exact first parent also reproduces the lock drift:

```text
$ cp Cargo.lock /private/tmp/asma-8187-seq3-base.Cargo.lock.committed
$ cargo generate-lockfile
$ cmp /private/tmp/asma-8187-seq3-base.Cargo.lock.committed Cargo.lock
FAIL: files differ at char 8342, line 337
```

Workspace clippy's identical `observed_identity` compile failure was already
reproduced against the production-identical sequence-2 base and is unchanged by
the evidence-only first-parent commit.

### Merged upstream parent `2e4b9985...`

An isolated archive of the exact upstream parent reproduced all three newly
observed merged-baseline failures:

```text
$ cargo test -p kontor-daemon --test loopback_api a_session_key_must_be_a_stable_client_message_id -- --exact
FAIL: HTTP 200 != 400 at loopback_api.rs:3098

$ cargo test -p kontor-api --test openapi_contract
FAIL: 2 passed, 1 failed; committed contract differs from served contract

$ cargo fmt --all -- --check
FAIL: identical rustfmt diff at team_contract.rs:467
```

## Independent mutation verification

Each mutant was seeded alone in an isolated archive. The candidate worktree was
never mutated. Sources were restored and compared byte-for-byte afterward.

| Mutant | Exact focused test | Result |
| --- | --- | --- |
| Remove preview-hash/CAS comparison | `a_core_team_succession_refuses_a_preview_whose_seat_revision_moved` | **Killed:** 200 instead of 400 |
| Remove immediate stale-native headroom preflight | `stale_core_team_succession_refuses_when_capacity_lapsed_after_its_preview` | **Killed:** 200 instead of 409 |
| Remove remediation occupancy-generation equality | `advance_and_remediate_judge_the_key_before_the_revision` | **Killed:** 200 instead of 409 |
| Omit atomic succession-ledger write | `a_succession_lost_after_its_store_commit_converges_on_one_receipt` | **Killed:** committed succession absent |
| Replace server-derived account authority with a constant | `a_core_team_succession_refuses_an_approved_route_authority_that_moved` | **SURVIVED:** 1 passed (F-8187-V8) |
| Persist predecessor seat id as placement container id | `an_exact_replay_after_the_receipt_landed_answers_from_durable_evidence` | **SURVIVED:** 1 passed (F-8187-V7) |

Mutation score for the six named high-risk cases is **4/6 killed (66.7%)**.
Both survivors target explicit scope evidence and remain blocking.

## Required correction before another verification

1. Return the complete canonical preview intent document beside its hash.
2. Persist and assert exact native project/workspace, container generation and
   provider correlation evidence in durable readback.
3. Isolate account-authority drift from headroom drift and kill the authority
   mutant.
4. Record and push one exact candidate in `HIGH-CHANGE-RECORD.md` before
   requesting verification.
5. Bring the integrated candidate's generated OpenAPI/client artifacts and
   formatting green, or land an upstream baseline that does so, then repeat the
   exact-pushed checks.

No gate was recorded and no workflow state was settled by this verification
turn.
