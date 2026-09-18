# ASMA-8187 high-verification report

Date: 2026-09-19
Artifact: `high-verification-report`
Task: Jira `ASMA-8187` / Kontor `01a0a943-652b-79d0-9f0c-f4965a33d7ab`
Phase: `high-verification`
Verification sequence: 4
Exact pushed candidate: `8c3a60c1c81a0ae3b08b8f0a72f5462ff84d5bfe`
Exact candidate parent: `6da164a99ded07182d494a0b2f18db5ca2fb3aa1`
Prior verification report commit: `7ce988a3778643d7bb0c695231f16289b0d3ce23`
Scope: [`HIGH-SCOPE-RECORD.md`](HIGH-SCOPE-RECORD.md)
Implementation evidence: [`HIGH-CHANGE-RECORD.md`](HIGH-CHANGE-RECORD.md)
Status: **FAIL**

## Verdict

The exact pushed candidate fails high verification.

V6 is closed: the public preview returns the canonical intent document, its
schema version and the hash taken over those canonical bytes. The focused Core
Team, stale-native, lost-ack, replay, generation/history and 12-case fence
matrix all pass. Schema 106 applies as an ordered 100–106 line and the full
schema suite passes. The bounded ASMA-7869 capability preserves the logical
seat, performs no launch, refuses active/history/effect-receipt shapes, forbids
OpenCode as the destination and replays one receipt.

Four blockers remain:

1. **F-8187-V7 remains open.** The exact former placement mutant still survives:
   storing the predecessor native as `placement.container_native_id` leaves the
   named durable-replay test green.
2. **F-8187-V8 remains open.** The exact former route-authority mutant still
   survives: replacing the uniquely resolved account id with a constant leaves
   the named authority-drift test green because apply independently refuses on
   the now-ambiguous immediate-headroom lookup.
3. **F-8187-V9 is new.** The correlation-challenge evidence validator is an
   ASMA-8118 document decoder, not a generic approved-evidence validator. It
   cannot validate an ASMA-8187 approved envelope.
4. **F-8187-V10 is new.** The ASMA-7869 supersession table declares a one-time
   receipt binding, but production code has no binder and the tests never prove
   that `receipt_id` becomes non-null. The response replay is stable, but its
   durable supersession evidence remains unbound to that receipt.

The store all-target suite and touched-crate clippy are additionally red on one
compile error in `hosted_seat_autonomy.rs`. The identical E0061 reproduces on
the exact candidate parent, so it is inherited and is not presented as an
ASMA-7869-tip regression or hidden as a pass.

No gate, production runtime, topology or task state was mutated. All mutants and
generated-artifact checks ran in isolated archives and were restored byte-for-
byte.

## Open-question ledger

- **Subject:** exact approved ASMA-8187 correlation-evidence revision and
  canonical envelope bytes.
- **Attached record:** this verification report, finding F-8187-V9.
- **Why ambiguous:** the source candidate contains only the legacy ASMA-8118
  evidence fixture and validator. The approved ASMA-8187 memory revision named
  during settlement is not materialized in this repository, so its revision id,
  content hash and report checksum cannot be evidenced from the candidate.
- **Options observed:** read the actual approved ASMA-8187 revision at operator
  time through the existing project-scoped memory repository; invent a new
  envelope; or borrow an unrelated ASMA-8188 envelope.
- **Disposition:** only the first option is admissible. No ASMA-8188 envelope is
  created, accepted or proposed by this report. Implementation and tests must
  use a synthetic generic-v1 fixture until the operator supplies the exact live
  approved revision through the existing request fields.

## Candidate identity and review boundary

```text
$ git rev-parse HEAD
8c3a60c1c81a0ae3b08b8f0a72f5462ff84d5bfe

$ git rev-parse HEAD^
6da164a99ded07182d494a0b2f18db5ca2fb3aa1

$ git status --short
<empty before this report edit>

$ git ls-remote --heads origin refs/heads/feat/ASMA-8187-repair-admin-preview-apply-stale-native-core-team-succession
8c3a60c1c81a0ae3b08b8f0a72f5462ff84d5bfe refs/heads/feat/ASMA-8187-repair-admin-preview-apply-stale-native-core-team-succession
```

The knowledge-graph project was consulted first. A forced refresh failed in its
worker, while `index_status` read back the exact candidate head. Coverage then
reported the relevant Rust files as metadata-changed and migration 0106 as not
tracked. Structural discovery used the graph where current; every cited changed
file and all SQL were read directly. `detect_changes` from the prior report
commit reported 83 changed files, 935 seed symbols and 1,975 impacted symbols.

## Scope findings

### F-8187-V6 — closed: canonical preview intent is public and tested

`core_team_route_plan` builds one canonical document containing the logical
binding, occupancy, predecessor, ECP placement, topology, role, frozen pins,
server-derived approved-route digest, headroom and desired route. It takes the
preview hash from that document and `preview_core_team_route` parses the same
canonical bytes back into `preview_intent`, with schema version 1.

An isolated mutant returned `{}` instead of the canonical document. The exact
authority test failed at its assertion that `preview_intent.approved_route`
equals the exposed approved-route digest. V6 is therefore closed.

### F-8187-V7 — blocking: exact container-native placement is still not tested

Production constructs `container_native_id` from the persisted container at
`crates/kontor-daemon/src/applications.rs:7891`. The named replay test at
`crates/kontor-daemon/tests/loopback_api.rs:55934` checks only that this field is
a string. It proves the independently persisted native parent, but not the
native workspace/container id itself.

The isolated mutant changed only:

```text
container_native_id: plan.container.identity.native_id.clone()
=>
container_native_id: plan.predecessor.native_identity.native_id.clone()
```

`an_exact_replay_after_the_receipt_landed_answers_from_durable_evidence`
remained green: **1 passed, 0 failed**. The previous V7 mutation survivor is
therefore still present.

Required correction: compare the returned and replayed
`container_native_id`, container generation, runtime kind, host and canonical
cwd against an independent persisted container-binding readback, not merely
their types or the readback document's own hash.

### F-8187-V8 — blocking: authority drift remains confounded by headroom

`resolved_route_authority` records the unique account id in the route digest.
The test enables a second selectable account after preview. Although it does not
seed a provider report for that account, apply calls `approved_route_account`
before using the pinned report; two selectable accounts therefore refuse on
headroom attribution independently of the route digest.

The isolated mutant changed the unique-account result to the constant
`fixed-account-authority`. The exact test
`a_core_team_succession_refuses_an_approved_route_authority_that_moved` remained
green: **1 passed, 0 failed**. Its fresh-preview assertion is conditional on a
200 response, so the ambiguous fresh preview does not expose the surviving
constant.

Required correction: stage authority drift without making
`approved_route_account` ambiguous at apply—for example, replace the sole
enabled account with another account that has separately valid pinned headroom,
then assert both the recomputed route digest mismatch and the exact refusal
path. The test must fail when the unique account id is constant even though all
other apply fences remain satisfiable.

### F-8187-V9 — blocking: correlation evidence is hard-coded to ASMA-8118

`correlation_challenge_evidence` at
`crates/kontor-daemon/src/applications.rs:16919` verifies the approved revision
id and canonical document hash, but then hard-codes:

- `/asma_8118_paseo_0_8_correlation_addendum_20260914`;
- `/asma_8118_binding_identity_correction_20260914`;
- epoch 2, end sequence 385, user positions `[1, 144]` and Paseo 0.8.0;
- historical report hash `3f667be8…` and correction hash `0ad93292…`;
- `/closeout_recovery_20260914/asma_8118/artifact`.

The only end-to-end fixture reproduces that exact ASMA-8118 document. An
otherwise valid ASMA-8187 approved envelope has no route through this validator.
This blocks use of the deployed recovery surface for the settlement now in
scope.

#### Smallest safe genericization

Keep the endpoint, request DTO, OpenAPI and durable challenge schema unchanged.
Refactor only the approved-evidence decoder:

1. Define a closed, typed `turn_correlation_challenge_evidence` envelope with
   `schema_version = 1` at one fixed root inside an approved memory document.
2. Decode either that generic v1 envelope or the exact legacy ASMA-8118 shape.
   The legacy path remains byte-exact; it is not weakened into aliases.
3. Do not accept a caller-supplied JSON pointer, task key, report key or
   historical coordinate. The caller continues to name only revision id,
   canonical content hash, report checksum and artifact.
4. Compare the generic envelope to authoritative current state with all of
   these exact fences:
   - project id and evidence purpose;
   - task id and task revision;
   - TeamRun id;
   - AgentRun id, revision and role slot;
   - active topology SeatBinding id;
   - runtime binding id, runtime kind, host, generation and native id;
   - blocker code `runtime_proof_unavailable` and `settlement_attempted=false`;
   - exact artifact key and report checksum;
   - a complete canonical timeline record with its digest, native epoch/end,
     terminal `next=null`, at least two exact user positions, and explicit
     absence of message id and native event id on every event.
5. Continue placing all live identities, revisions, evidence hashes and the
   newly observed boundary in the existing preview hash.

Required tests: legacy ASMA-8118 still passes; a generic ASMA-8187 fixture
passes; every identity/evidence field above fails independently; foreign or
mislabelled evidence fails; preview sends nothing; lost acknowledgement and
same-key reconciliation still send only once. No ASMA-8188 envelope is needed
or permitted.

### F-8187-V10 — blocking: supersession evidence never binds its receipt

Migration 0106 adds nullable `receipt_id` and a trigger allowing exactly one
NULL-to-value update (`0106_core_team_route_succession_recovery.sql:172-200`).
`supersede_core_team_launch_intent` records the command receipt after the atomic
swap, but no store method or update binds that id to
`hosted_seat_launch_intent_supersessions`. An exhaustive source search found no
writer beyond the INSERT of NULL. The replay test compares response receipt ids
only; it never reads the supersession row's `receipt_id`.

Required correction: add a one-time exact-key/intent-hash receipt binder,
invoke it after `record`, invoke it again on lost-ack replay when the field is
NULL, and assert the stored receipt id equals the returned command receipt. A
different receipt or a second binding must refuse.

## ASMA-7869 bounded supersession result

The capability otherwise matches the bounded request:

- the bounded contract explicitly names logical SeatBinding
  `01a02b8e-8f63-7161-b043-cf8cc6d1297e`; the isolated regression fixture
  models the same prepared generation-1 OpenCode shape with no active occupancy
  or history, without reading or mutating live topology;
- the successful case leaves the same logical SeatBinding active at the same
  revision, with no release or replacement and no native occupancy;
- installed/observed, history, active occupancy, binding revision, generation,
  route and prepared-time drift all refuse;
- an effect receipt naming the seat refuses, while read-only/caller-only
  receipts are deliberately not treated as effects;
- the destination must be a different, governed, non-OpenCode route;
- same-key replay returns the same receipt, changed intent conflicts and a
  second supersession of the occupancy refuses;
- no OpenCode launch and no new SeatBinding are produced.

Independent mutants disabling the occupancy/history fence, effect-receipt fence
and explicit OpenCode destination ban were all killed by the named tests. The
implementation record's reported single route-predicate survivor is honest in
shape: the migration trigger independently carries the same route evidence, so
removing only the store predicate does not admit the defect. This verification
does not inflate that double-guarded survivor into a kill.

## Schema 100–106 result

Runtime ordering is coherent: `SCHEMA_VERSION` is 106, every migration 100
through 106 is registered once in order, each file ends at its matching
`PRAGMA user_version`, and the exact table inventory includes both
`core_team_route_successions` and
`hosted_seat_launch_intent_supersessions`.

```text
$ cargo test -p kontor-store --test schema_v1
PASS: 60 passed, 0 failed; 175.43s
```

Documentation headers in 0102 and 0103 still say schema v99 and v100 while
their files end at 102 and 103. That is stale explanatory text, not executable
migration drift, but it should be corrected with the next candidate.

## Exact commands and results

### Focused scope and replay

```text
$ cargo test -p kontor-daemon --test loopback_api core_team -- --nocapture
PASS: 18 passed, 0 failed

$ cargo test -p kontor-daemon --test loopback_api succession -- --nocapture
PASS: 11 passed, 0 failed

$ cargo test -p kontor-daemon --test loopback_api launch_intent_supersession -- --nocapture
PASS: 3 passed, 0 failed

$ cargo test -p kontor-daemon --test loopback_api \
    a_never_bound_prepared_launch_intent_is_superseded_in_place -- --exact --nocapture
PASS: 1 passed, 0 failed

$ cargo test -p kontor-daemon --test loopback_api
PASS outside sandbox: 404 passed, 0 failed, 1 ignored; 41.41s
```

The first full loopback run inside the filesystem sandbox reported 29 failures,
all at Wiremock's local-port bind with `PermissionDenied`. The exact suite was
rerun with loopback binding allowed and passed as above. The daemon library's
single Wiremock failure was likewise rerun exactly and passed.

### Touched crates

```text
$ SWAGGER_UI_DOWNLOAD_URL=file://<cached-v5.17.14.zip> cargo test \
    -p kontor-api -p kontor-core -p kontor-runtime \
    -p kontor-runtime-paseo -p kontor-mcp
PASS: all executed suites green; Paseo contract 219 passed, 6 live tests ignored

$ cargo test -p kontor-api -p kontor-core -p kontor-runtime \
    -p kontor-runtime-paseo -p kontor-store -p kontor-mcp
FAIL at compile: E0061 in hosted_seat_autonomy.rs:368
```

### Generated OpenAPI and client parity

Run in an isolated `git archive` of exact HEAD:

```text
$ KONTOR_UPDATE_CONTRACT=1 CARGO_TARGET_DIR=<candidate-target> \
    cargo test -p kontor-api --test openapi_contract
PASS: 3 passed, 0 failed

$ cmp -s <checkout>/crates/kontor-api/contract/openapi.json \
    <archive>/crates/kontor-api/contract/openapi.json
PASS: exit 0

$ ./node_modules/.bin/openapi-typescript \
    ../../crates/kontor-api/contract/openapi.json -o src/api/schema.d.ts
PASS: openapi-typescript 7.13.0

$ cmp -s <checkout>/apps/console/src/api/schema.d.ts \
    <archive>/apps/console/src/api/schema.d.ts
PASS: exit 0
```

The `pnpm --filter kontor-console generate:api` wrapper in the archive attempted
an install and stopped with `ERR_PNPM_ABORTED_REMOVE_MODULES_DIR_NO_TTY`; the
exact underlying committed generator was then run directly against the
archive's contract and reproduced the client byte-for-byte.

### Formatting and clippy

```text
$ cargo fmt --all -- --check
PASS: exit 0

$ cargo clippy -p kontor-api -p kontor-core -p kontor-daemon -p kontor-mcp \
    -p kontor-runtime -p kontor-runtime-paseo -p kontor-store \
    --all-targets -- -D warnings
FAIL at compile: the same hosted_seat_autonomy.rs E0061
```

## Exact inherited-baseline reproduction

Candidate and exact parent were both exercised with the same focused command:

```text
$ cargo test -p kontor-store --test hosted_seat_autonomy --no-run

8c3a60c1…: FAIL E0061 at hosted_seat_autonomy.rs:368 — method expects six
arguments, test supplies four.

6da164a99ded07182d494a0b2f18db5ca2fb3aa1: identical FAIL E0061 at the same
file, line, call and missing `Option<&HostedSeatRouteFence>` /
`Option<&NewCoreTeamRouteSuccession>` arguments.
```

This failure is inherited by commit `8c3a60c1`; it is not caused by the bounded
ASMA-7869 delta. It still means the integrated candidate's all-target store and
clippy gates are red.

## Independent mutation record

Each mutant was seeded alone in an isolated exact-HEAD archive. Baseline files
were SHA-256 checked before mutation and after restoration:

```text
applications.rs afacd9f62edbbbba2fdc23203d9c184cbdbe1c85dc1c9ae71669eebce60d2d44
repository.rs   15cc9c8ca02e80bcbfb75188877ba14e5be8471890bd8175d15ba8eaa89dcbe6
```

| Mutant | Focused test | Result |
| --- | --- | --- |
| Return `{}` instead of canonical `preview_intent` | authority-drift case | **KILLED** |
| Persist predecessor native as placement `container_native_id` | exact replay after receipt | **SURVIVED** — F-8187-V7 |
| Replace unique account authority with a constant | authority-drift case | **SURVIVED** — F-8187-V8 |
| Disable active-occupancy/history absence fence | non-inert shape matrix | **KILLED** |
| Disable effect-receipt fence | effect-receipt case | **KILLED** |
| Remove explicit OpenCode destination ban | non-inert shape matrix | **KILLED** |

Independent score: **4/6 killed (66.7%)**. The two survivors are both required
corrections and are not discounted by the green baseline suite.

## Required correction before re-verification

1. Make V7 prove the exact native container identity from independent persisted
   placement evidence; rerun the same mutant.
2. Isolate V8 authority drift from immediate-headroom ambiguity; rerun the same
   constant-authority mutant.
3. Add the generic v1 correlation-evidence decoder and exact fence matrix while
   retaining the byte-exact legacy ASMA-8118 adapter. Do not add or borrow an
   ASMA-8188 envelope.
4. Bind the ASMA-7869 supersession row to its command receipt and test lost-ack
   repair of that binding.
5. Repair or explicitly integrate the inherited all-target store compile break,
   then rerun store and touched-crate clippy.
