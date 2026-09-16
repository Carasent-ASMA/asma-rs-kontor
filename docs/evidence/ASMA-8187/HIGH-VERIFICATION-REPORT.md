# ASMA-8187 high-verification report

Date: 2026-09-16
Artifact: `high-verification-report`
Task: Jira `ASMA-8187` / Kontor `01a0a943-652b-79d0-9f0c-f4965a33d7ab`
Phase: `high-verification`
Candidate: `1fcd42d61b2a93ad6ee1f344d0f82f634dbc2381`
Exact base: `52c9c4efc48397142b2b3df536fb59bef7ca57fa`
Scope: [`HIGH-SCOPE-RECORD.md`](HIGH-SCOPE-RECORD.md)
Implementation evidence: [`HIGH-CHANGE-RECORD.md`](HIGH-CHANGE-RECORD.md)
Status: **FAIL — the candidate does not satisfy the recorded high-scope contract**

## Verdict

The exact pushed candidate is rejected for high verification. Its implemented
stale-native path, focused headroom/provider fences, generation/history behavior,
lost archive acknowledgement, lost launch acknowledgement, existing replay path,
remediation-generation authority, touched crates, and generated contracts all
pass their checked-in tests. The three named mutants were independently killed.

Those green results do not close the authoritative scope. Source inspection and
the public contract establish four blocking gaps:

1. the committed store replacement precedes the command receipt, with no
   recovery path for an acknowledgement/process loss in that interval;
2. preview/apply does not expose or fence the required live server-derived
   approved model-route digest or the pinned Team Definition id/version/digest;
3. apply/readback and its command receipt do not contain the required complete
   two-generation identity, placement, route, pin and final-readback evidence;
4. the required one-by-one mismatch and store-replacement/receipt-persistence
   recovery cases are not present.

No production code was changed during this verification. Mutations were made
only in an isolated candidate snapshot, observed red, restored byte-for-byte and
rerun green. Generated contract checks left no diff. The only retained worktree
change from this turn is this report.

## Candidate identity and boundary

The target repository was clean at handoff and resolved as follows:

```text
$ git rev-parse HEAD
1fcd42d61b2a93ad6ee1f344d0f82f634dbc2381

$ git rev-parse HEAD^
52c9c4efc48397142b2b3df536fb59bef7ca57fa

$ git ls-remote --heads origin refs/heads/feat/ASMA-8187-repair-admin-preview-apply-stale-native-core-team-succession
1fcd42d61b2a93ad6ee1f344d0f82f634dbc2381	refs/heads/feat/ASMA-8187-repair-admin-preview-apply-stale-native-core-team-succession
```

`git diff --stat HEAD^..HEAD` reports 14 files, 2,385 insertions and 51
deletions. Review covered the complete candidate delta: API/OpenAPI/client,
core/store/runtime/Paseo adapter, daemon flow, focused tests and the change
record. The codebase knowledge graph was consulted first; its index metadata
matched every touched source file and its candidate-vs-base impact scan reported
87 changed seed symbols and 630 impacted symbols across the daemon, store,
runtime, API, MCP and client surfaces.

## Blocking findings

### F-8187-V1 — high: store replacement can commit without a reconstructable command receipt

The successful succession launches the native, calls
`replace_hosted_topology_seat_route`, updates the binding observation, and only
then records the command receipt (`crates/kontor-daemon/src/applications.rs` lines
21652-21723). The store method commits the history append and active-row
replacement in its own transaction at
`crates/kontor-store/src/repository.rs:4457-4605`. Therefore a process loss after
line 21699 or 21713 and before `self.record` at 21716 leaves durable generation-2
state with no receipt for the idempotency key.

Replay checks the receipt first (`applications.rs:21351-21364`), but has no
incomplete-operation ledger to find when that receipt is absent. It then
re-plans against the moved SeatBinding/occupancy and the spent preview, so it
cannot reconstruct and persist the same receipt required by the scope.

The checked-in test
`a_committed_succession_admits_no_second_transition_through_either_door`
(`loopback_api.rs:47350-47450`) begins only after the request returned 200 and
the receipt already exists. Proving that a completed receipt is undeletable does
not make the earlier store-commit-before-receipt interval unreachable. The
required injected lost acknowledgement after store replacement is absent.

Required correction: make the store transition and durable recovery evidence
atomic enough to resume, or persist an operation record that lets the same key
reconstruct the exact receipt from generation-1 history and generation-2 active
occupancy. Retain a deterministic fault injection immediately after store
replacement and prove replay returns the same successor and receipt with no
second native/store effect.

### F-8187-V2 — high: required approved-route and Team Definition authority is absent from the fence

The preview document at `applications.rs:7030-7094` includes the caller's parsed
`desired` route, role-catalog data, topology data, a resolved Core Team digest,
completion profile and headroom. It does not include a separately server-derived
approved model-route digest. `desired_model_route` originates in the request
(`crates/kontor-api/src/applications.rs:895-909`); matching it to the stored
predecessor during stale recovery is not the scope's live approved-route
attestation.

The same document contains no Team Definition id, version or canonical digest.
Role-catalog, topology and resolved Core Team pins are distinct authorities and
do not substitute for the explicit Team Definition fence in the scope record.
Repository search found Team Definition pins in other daemon operations but not
in `core_team_route_plan`.

Required correction: derive both authorities from current server state, expose
the approved route digest in preview, bind the exact Team Definition triple and
route digest into the canonical preview intent, reread them on apply, and add
focused drift refusals with zero effects.

### F-8187-V3 — high: post-apply readback and receipt are materially incomplete

`CoreTeamRouteOutcomeDto` exposes only the current Core Team projection, logical
SeatBinding id, predecessor native id, successor native id and generic mutation
receipt (`crates/kontor-api/src/applications.rs:1024-1040`). It does not return
both occupancy generations, complete predecessor/successor native identities,
provider sessions, exact ECP placement/cwd, unchanged route and pins, or a final
readback digest.

The command intent recorded at `applications.rs:21351-21359` binds only project,
epic, SeatBinding, predecessor native id and preview hash. The generic receipt
created at `:21716-21740` does not itself bind predecessor outcome, successor
outcome, store transition or final readback digest as required. On replay, the
response reconstructs one successor native id from history/current occupancy
and returns the current Core Team projection (`:21363-21452`), not the original
complete final readback.

Required correction: persist and return the exact scope-defined readback, bind
its digest and transition outcomes to the receipt, and prove exact same-key
replay after later state movement returns that command's own durable evidence.

### F-8187-V4 — high: the required refusal/recovery matrix is incomplete

The checked-in suite has strong behavior cases for headroom, SeatBinding
revision, one topology representative, one binding representative, runtime
busy/permission state, archive/launch acknowledgement loss and later-generation
replay. It does not independently drift and assert zero effects for every named
scope fence: occupancy generation; predecessor runtime generation and provider
session; ECP project/workspace/cwd/container generation/provider correlation;
approved route and digest; Team Definition digest; each topology/Core Team pin;
and completion-profile digest. Some fields are structurally included in the
preview hash, but the scope explicitly requires focused runnable cases for each
mismatch, and V2 identifies mandatory fields that are not included at all.

There is also no injected loss after store replacement or after receipt
persistence. The former is a live defect (V1); the latter is needed to prove
that exact replay performs no runtime, headroom or store work and returns the
same complete readback.

Required correction: retain the complete named mismatch table as focused tests,
including exact zero-effect assertions, and all four ordered lost-ack injection
points from the scope record.

## Scope behavior exercised successfully

The following candidate behaviors were independently exercised and passed:

- stale-native succession keeps the logical SeatBinding, writes the predecessor
  once to immutable history, installs one generation-2 active occupant and uses
  a generation-2-scoped credential;
- missing, stale, exhausted, wrong-account and ambiguous-account provider
  evidence refuses; capacity lapsed after preview refuses before archive;
- a moved SeatBinding revision and representative topology/binding drift refuse;
- a running/non-idle or permission-blocked predecessor refuses;
- lost archive acknowledgement and lost launch acknowledgement converge without
  a duplicate successor in the tested intervals;
- exact replay resolves its own generation-2 successor after the seat later
  advances;
- generation-2 remediation authority passes while generation-1 authority is
  rejected at submission time;
- generated OpenAPI and TypeScript client artifacts reproduce byte-for-byte.

These results verify the behavior implemented by the candidate; they do not
waive the missing high-scope evidence and recovery contracts above.

## Exact command and result record

### Focused stale-native, CAS, replay and remediation tests

```text
$ cargo test -p kontor-daemon --test loopback_api stale_core_team_succession -- --nocapture
PASS: 4 passed, 0 failed

$ cargo test -p kontor-daemon --test loopback_api a_core_team_succession -- --nocapture
PASS: 2 passed, 0 failed

$ cargo test -p kontor-daemon --test loopback_api a_core_team_correction_refuses_to_retire_a_predecessor_with_work_in_flight -- --exact
PASS: 1 passed, 0 failed

$ cargo test -p kontor-daemon --test loopback_api a_replayed_core_team_succession_returns_its_own_successor_after_the_seat_moves_on -- --exact
PASS: 1 passed, 0 failed

$ cargo test -p kontor-daemon --test loopback_api a_lost_archive_acknowledgement_converges_on_one_core_team_successor -- --exact
PASS: 1 passed, 0 failed

$ cargo test -p kontor-daemon --test loopback_api a_lost_launch_acknowledgement_recovers_the_same_core_team_successor -- --exact
PASS: 1 passed, 0 failed

$ cargo test -p kontor-daemon --test loopback_api a_committed_succession_admits_no_second_transition_through_either_door -- --exact
PASS: 1 passed, 0 failed; does not inject the pre-receipt interval described in F-8187-V1

$ cargo test -p kontor-daemon --test loopback_api advance_and_remediate_judge_the_key_before_the_revision -- --exact
PASS: 1 passed, 0 failed
```

### Touched crates and contract parity

```text
$ cargo test -p kontor-api -p kontor-core -p kontor-runtime -p kontor-runtime-paseo -p kontor-store
PASS: exit 0; all unit/integration/doc tests passed; 6 live Paseo tests ignored by design

$ cargo test -p kontor-mcp
PASS: 65 passed, 0 failed (57 library, 3 binary, 5 seat-contract tests)

$ KONTOR_UPDATE_CONTRACT=1 cargo test -p kontor-api --test openapi_contract
PASS: 3 passed, 0 failed

$ pnpm --filter kontor-console generate:api
PASS: openapi-typescript 7.13.0 generated apps/console/src/api/schema.d.ts

$ git diff --exit-code -- crates/kontor-api/contract/openapi.json apps/console/src/api/schema.d.ts
PASS: exit 0, no generated drift
```

The first touched-crate attempt was blocked by sandboxed DNS while resolving
Swagger UI. The permitted rerun above completed successfully and is the result
used for the verdict.

### Formatting, lint and repository-wide candidate run

```text
$ cargo fmt --all -- --check
PASS: exit 0

$ cargo clippy -p kontor-api -p kontor-core -p kontor-daemon -p kontor-runtime -p kontor-runtime-paseo -p kontor-store --all-targets -- -D warnings
PASS: exit 0

$ cargo clippy --workspace --all-targets -- -D warnings
FAIL: error[E0063], missing field `observed_identity` in initializer of `kontor_jira::jira::JiraResponse`
      at tests/e2e/pilot_sections/domain.rs:2468:5

$ cargo test -p kontor-daemon
RESULT: daemon library 81 passed; daemon binary 2 passed; account_pinning 5 passed;
        loopback_api 332 passed, 7 failed, 1 ignored; exit 101
```

The first full-daemon attempt was blocked only by sandbox loopback permission;
the permitted rerun produced the recorded result. The seven failures were:

```text
a_body_overtaken_during_a_lost_confirmation_is_refused
a_human_authored_body_is_preserved_until_replacement_is_authorized
a_publication_that_never_landed_is_still_refused
a_publication_whose_confirmation_was_lost_is_settled_by_refetch
an_epic_placeholder_body_is_typed_reported_and_repairable
reconcile_plan_refuses_to_call_a_placeholder_body_converged
replaying_a_partial_admission_delivers_its_durable_follow_up
```

The first six received HTTP 503 `unavailable` where their assertions expected
200. The last received HTTP 409 `revision_conflict` where it expected 200.

### Exact inherited-baseline reproduction

An isolated archive of exact parent
`52c9c4efc48397142b2b3df536fb59bef7ca57fa` was used; the candidate worktree was
not rewritten. Each daemon failure was run at the parent with:

```text
$ cargo test -p kontor-daemon --test loopback_api a_body_overtaken_during_a_lost_confirmation_is_refused -- --exact
$ cargo test -p kontor-daemon --test loopback_api a_human_authored_body_is_preserved_until_replacement_is_authorized -- --exact
$ cargo test -p kontor-daemon --test loopback_api a_publication_that_never_landed_is_still_refused -- --exact
$ cargo test -p kontor-daemon --test loopback_api a_publication_whose_confirmation_was_lost_is_settled_by_refetch -- --exact
$ cargo test -p kontor-daemon --test loopback_api an_epic_placeholder_body_is_typed_reported_and_repairable -- --exact
$ cargo test -p kontor-daemon --test loopback_api reconcile_plan_refuses_to_call_a_placeholder_body_converged -- --exact
$ cargo test -p kontor-daemon --test loopback_api replaying_a_partial_admission_delivers_its_durable_follow_up -- --exact
```

**BASE REPRODUCED:** all seven failed with the same HTTP/error classes and same
assertions as the candidate. They are inherited baseline failures, not hidden or
credited as candidate passes.

The workspace clippy failure was also reproduced on that exact parent with the
same command, error, field and source location. It is inherited.

```text
$ python3 scripts/verify-tree.py --mode archive
FAIL: Cargo.lock regeneration differs byte-for-byte from the committed lockfile

# exact-parent archive
$ cargo generate-lockfile
$ cmp Cargo.lock.committed Cargo.lock
FAIL: files differ at char 12736, line 519
```

The archive verifier initially required permitted network access. The exact
parent reproduces the same lockfile drift, so this is an inherited repository
baseline failure. It is not masked and is not evidence for or against the four
candidate-specific findings.

## Independent mutation verification

Each mutant was seeded alone in an isolated archive of candidate
`1fcd42d61b2a93ad6ee1f344d0f82f634dbc2381`. The original source was restored
after every red run; the three focused tests then passed together, the isolated
`applications.rs` compared byte-for-byte with the candidate, and no `MUTANT
ASMA-8187` marker remained.

| Mutant | Exact test command | Observed result |
| --- | --- | --- |
| remove `if plan.preview_hash != request.preview_hash` apply CAS comparison | `cargo test -p kontor-daemon --test loopback_api a_core_team_succession_refuses_a_preview_whose_seat_revision_moved -- --exact` | **killed:** test failed; apply returned 200 where 400 was required |
| remove the immediate stale-native headroom preflight before archive | `cargo test -p kontor-daemon --test loopback_api stale_core_team_succession_refuses_when_capacity_lapsed_after_its_preview -- --exact` | **killed:** test failed; apply returned 200 where 409 was required |
| remove the current occupancy/remediation-generation equality check | `cargo test -p kontor-daemon --test loopback_api advance_and_remediate_judge_the_key_before_the_revision -- --exact` | **killed:** test failed; generation-1 authority returned 200 where 409 was required |

Restored confirmation:

```text
$ cargo test -p kontor-daemon --test loopback_api a_core_team_succession_refuses_a_preview_whose_seat_revision_moved -- --exact
PASS: 1 passed, 0 failed

$ cargo test -p kontor-daemon --test loopback_api stale_core_team_succession_refuses_when_capacity_lapsed_after_its_preview -- --exact
PASS: 1 passed, 0 failed

$ cargo test -p kontor-daemon --test loopback_api advance_and_remediate_judge_the_key_before_the_revision -- --exact
PASS: 1 passed, 0 failed
```

## Exit criteria for another high-verification turn

1. Close F-8187-V1 with durable pre-receipt recovery and a retained injected
   loss immediately after store replacement.
2. Close F-8187-V2 with server-derived approved-route and Team Definition
   authority in preview/apply and individual zero-effect drift tests.
3. Close F-8187-V3 with a durable complete final readback and receipt digest
   that exact replay can return after later seat movement.
4. Close F-8187-V4 with every named mismatch and all four acknowledgement-loss
   points represented by focused runnable tests.
5. Re-run the focused tests, touched crates, MCP, OpenAPI/client regeneration,
   format, touched-crate clippy, full daemon suite, exact-base classification and
   all three named mutants on one exact new pushed candidate.

No gate was recorded and no workflow state was settled by this verification
turn.
