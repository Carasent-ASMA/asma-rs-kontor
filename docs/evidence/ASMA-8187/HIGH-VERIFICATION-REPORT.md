# ASMA-8187 high-verification report

Date: 2026-09-19
Artifact: `high-verification-report`
Task: Jira `ASMA-8187` / Kontor `01a0a943-652b-79d0-9f0c-f4965a33d7ab`
Verification sequence: 5
Exact pushed candidate: `7864a3ca8d86d7527bf200a0daf78d48363fd27e`
Exact candidate parent / rejected baseline: `8c3a60c1c81a0ae3b08b8f0a72f5462ff84d5bfe`
Requirements: `asma-8187-remediation-v7-v10-20260919` revision 1,
revision id `01a0b6be-79d3-7282-a37a-dfe7e0443c11`
Dispatch approval: `watchdog-standing-scope-approval-kontor-six-epics-20260919`
revision 1, approval receipt `01a0b706-1135-77a0-b350-6dab357477fa`
Scope: [`HIGH-SCOPE-RECORD.md`](HIGH-SCOPE-RECORD.md)
Implementation evidence: [`HIGH-CHANGE-RECORD.md`](HIGH-CHANGE-RECORD.md)
Status: **FAIL**

The SHA-256 of this report is recorded externally in the terminal handoff and
Kontor artifact delivery. It is not embedded in the file it hashes.

## Verdict

The exact pushed candidate fails high verification because two required
regression mutations survive its committed focused tests:

1. **F-8187-V11 — V9 retired-seat regression survivor.** Removing the
   `TopologyLifecycle::Active` fence from generic-v1 SeatBinding resolution
   leaves
   `a_generic_approved_correlation_envelope_fences_every_identity_independently`
   green. The candidate implementation does refuse a genuinely retired
   binding, but the durable test never retires the binding and therefore does
   not protect that requirement.
2. **F-8187-V12 — V10 lost-ack replay regression survivor.** Binding the
   supersession receipt only when the launch-intent swap returns
   `Applied::Updated`, and skipping the binder when an exact replay returns
   `Applied::Unchanged`, leaves
   `a_launch_intent_supersession_is_exactly_once_under_replay_and_drift` green.
   This mutant breaks the required recovery interval in which the swap and
   command receipt exist but the first acknowledgement is lost before the
   supersession row is bound.

The production implementation passed verifier-only augmented cases for both
behaviors: a valid generic-v1 envelope was refused after its exact SeatBinding
was retired, and a fault inserted after command-receipt recording but before
receipt binding was repaired by same-key replay with the original receipt.
Those temporary cases establish current behavior, not durable regression
coverage. The two surviving mutants therefore remain blocking.

V7 and V8 are corrected and their exact former mutants are killed. The
generic-v1 identity matrix, byte-exact legacy ASMA-8118 adapter, V10 receipt
binding, schema 100–106, generated OpenAPI/client parity, focused suites and
the full daemon loopback suite otherwise pass. The known store all-target
E0061 reproduces identically on the exact parent and is classified separately
as inherited; it does not mask the two new survivors.

No gate, topology, Jira, deployment, merge or task state was changed. No
ASMA-8188 envelope was created. All verifier-only tests and mutants ran in an
isolated archive and the three touched Rust files were restored byte-for-byte.

## Authority and identity readback

- Verifier AgentRun: `01a0a9f6-3a5d-7640-ba9b-63807b31ef70`, revision 8.
- TeamRun: `01a0a945-4a98-70b1-a16d-b087017edb0d`.
- Persistent SeatBinding: `01a0a946-b227-77a1-811b-a626d80f7abc`.
- Runtime binding: `01a0a9f6-3a5d-7640-ba9b-63807b31ef71`, generation 1.
- Native session: `8a00958e-34ec-46b0-b7d3-500fe33e2bdb`.
- TSW: `wks_810373445e9f4d2b`.
- Intended recipient: existing LSA native session
  `1a3be2ee-b48c-42b4-b181-ba4b4f8d527a`.

Kontor read back the task in `high-implementation` with the prior
high-verification gate rejected. The approval above authorizes this manual
verification dispatch; it is not treated as a verdict or gate mutation.

## Candidate identity

```text
$ git rev-parse HEAD
7864a3ca8d86d7527bf200a0daf78d48363fd27e

$ git rev-parse HEAD^
8c3a60c1c81a0ae3b08b8f0a72f5462ff84d5bfe

$ git ls-remote --heads origin \
    refs/heads/feat/ASMA-8187-repair-admin-preview-apply-stale-native-core-team-succession
7864a3ca8d86d7527bf200a0daf78d48363fd27e refs/heads/feat/ASMA-8187-repair-admin-preview-apply-stale-native-core-team-succession

$ git status --short
<empty before report authoring>
```

The delta from the rejected baseline changes five files: daemon production and
loopback tests, store production, the change record and this report. A forced
knowledge-graph refresh failed in its worker; `detect_changes` still identified
those five changed files, while direct reads of the exact checkout were used
for all candidate conclusions.

## Requirement results

### V7 — corrected; named mutant killed

Production persists `plan.container.identity.native_id` as
`placement.container_native_id`. The durable replay test now independently
reads the persisted topology container and compares native id, binding id,
runtime kind, host, generation and cwd; it also excludes predecessor and
successor identities.

Mutant: replace the container native id with the predecessor native id.
Result: **KILLED**; expected `native-container-2`, observed mutant value
`native-hosted-seat-4`.

### V8 — corrected; named mutant killed

The authority test disables the initially selected account, enables a second
account with valid headroom, requires a fresh preview to succeed and requires
its digest to differ. Apply receives valid replacement headroom, isolating the
route-authority fence from the provider-headroom fence.

Mutant: return constant account authority `fixed-account-authority`.
Result: **KILLED** by the unconditional fresh-preview digest inequality.

### V9 — production behavior passes; required regression mutant survives

The fixed generic-v1 root is `/turn_correlation_challenge_evidence`. Typed
`deny_unknown_fields` decoding fences schema and purpose, project, task id and
revision, TeamRun, AgentRun id/revision/role, active topology SeatBinding,
runtime binding id/kind/host/generation/native, exact blocker state,
artifact/report checksum, timeline digest, epoch, terminal `next=null`, user
positions and explicit null message/native-event identities.

The legacy ASMA-8118 root and exact adapter remain separate. The existing
legacy fixture passes without translation through generic-v1. There is no
ASMA-8188 input or fallback.

Named mutant: omit the generic runtime-generation comparison. Result:
**KILLED** at the `runtime-generation` case.

Supplemental mutant: omit only the active-lifecycle predicate during topology
SeatBinding resolution. Result: **SURVIVED** the committed generic-v1 test,
1 passed, 0 failed. A verifier-only test that retired the actual binding killed
the same mutant and passed against restored production. This is F-8187-V11.

### V10 — production behavior passes; required lost-ack mutant survives

The store binds a receipt by exact idempotency key and intent hash, permits an
identical rebind as unchanged and refuses a foreign receipt. The daemon invokes
the binder after `record` on every call. Schema 106 permits exactly the intended
NULL-to-value transition.

Named mutant: remove the daemon binder call. Result: **KILLED** by the committed
replay test's stored-receipt assertion.

Supplemental mutant: call the binder only for `Applied::Updated`, skipping it
for exact `Applied::Unchanged` replay. Result: **SURVIVED** the committed V10
test, 1 passed, 0 failed. A verifier-only fault seam after `record` and before
the binder proved that restored production repairs NULL to the original receipt
on replay and returns the same receipt on a third replay. This is F-8187-V12.

The bounded ASMA-7869 path otherwise preserves logical SeatBinding
`01a02b8e-8f63-7161-b043-cf8cc6d1297e`; refuses native occupancy, history,
effect receipts, installed or observed intents and all CAS drift; creates no
new SeatBinding; and forbids OpenCode as the replacement route.

## Exact commands and results

All authoritative candidate results below used an isolated candidate-only
target directory after a shared-target contamination was detected and
discarded.

```text
$ CARGO_TARGET_DIR=/private/tmp/asma8187-candidate-target \
    cargo test -p kontor-daemon --test loopback_api core_team -- --nocapture
PASS: 18 passed, 0 failed, 388 filtered out

$ CARGO_TARGET_DIR=/private/tmp/asma8187-candidate-target \
    cargo test -p kontor-daemon --test loopback_api succession -- --nocapture
PASS: 11 passed, 0 failed, 395 filtered out

$ CARGO_TARGET_DIR=/private/tmp/asma8187-candidate-target \
    cargo test -p kontor-daemon --test loopback_api launch_intent -- --nocapture
PASS: 4 passed, 0 failed, 402 filtered out

$ CARGO_TARGET_DIR=/private/tmp/asma8187-candidate-target \
    cargo test -p kontor-daemon --test loopback_api correlation -- --nocapture
PASS: 1 passed, 0 failed, 405 filtered out

$ CARGO_TARGET_DIR=/private/tmp/asma8187-candidate-target \
    cargo test -p kontor-daemon --test loopback_api \
    an_ambiguous_history_only_settles_after_one_server_owned_challenge -- --exact
PASS: 1 passed, 0 failed, 405 filtered out

$ CARGO_TARGET_DIR=/private/tmp/asma8187-candidate-target \
    cargo test -p kontor-daemon --test loopback_api
PASS outside sandbox: 405 passed, 0 failed, 1 ignored; 41.30s
```

### Schema 100–106

Direct inspection found `SCHEMA_VERSION = 106`, migrations 0100 through 0106
registered once and in order, and each migration tail setting its matching
`PRAGMA user_version`.

```text
$ CARGO_TARGET_DIR=/private/tmp/asma8187-candidate-target \
    cargo test -p kontor-store --test schema_v1
PASS: 60 passed, 0 failed; 177.17s
```

### Generated OpenAPI and client parity

Run from an isolated `git archive` of exact source head, using the cached
Swagger UI archive and the workspace's committed generator version:

```text
$ KONTOR_UPDATE_CONTRACT=1 CARGO_TARGET_DIR=<candidate-target> \
    cargo test -p kontor-api --test openapi_contract
PASS: 3 passed, 0 failed

$ cmp -s <archive>/crates/kontor-api/contract/openapi.json \
    <checkout>/crates/kontor-api/contract/openapi.json
PASS: exit 0

$ <workspace>/apps/console/node_modules/.bin/openapi-typescript \
    ../../crates/kontor-api/contract/openapi.json -o src/api/schema.d.ts
PASS: openapi-typescript 7.13.0

$ cmp -s <archive>/apps/console/src/api/schema.d.ts \
    <checkout>/apps/console/src/api/schema.d.ts
PASS: exit 0

$ cargo fmt --all -- --check
PASS: exit 0
```

### Exact inherited-baseline classification

```text
$ CARGO_TARGET_DIR=/private/tmp/asma8187-candidate-target \
    cargo test -p kontor-store --test hosted_seat_autonomy --no-run
7864a3ca8d86d7527bf200a0daf78d48363fd27e: FAIL E0061 at
hosted_seat_autonomy.rs:368; method expects six arguments, test supplies four.

$ cargo test -p kontor-store --test hosted_seat_autonomy --no-run
8c3a60c1c81a0ae3b08b8f0a72f5462ff84d5bfe: identical E0061 at the same
file, line, call and missing HostedSeatRouteFence/CoreTeamRouteSuccession args.

$ cargo clippy -p kontor-api -p kontor-core -p kontor-daemon -p kontor-mcp \
    -p kontor-runtime -p kontor-runtime-paseo -p kontor-store \
    --all-targets -- -D warnings
FAIL: the same inherited E0061
```

This remains an integrated all-target/clippy failure, but it is not introduced
by candidate `7864a3ca…` and is not substituted for either new mutation result.

## Independent mutation record

Each mutant was seeded alone in an isolated exact-head archive. The source was
restored after every run. Final SHA-256 readback matched the original:

```text
applications.rs 37a9ff523e435eb779ba6734dcd464f74639ff71f46852282b9338890b2ba10f
loopback_api.rs b010a7748080346443d348bb666884026fd63b535bd965a9cc256b81285b706c
repository.rs 18a7e9e32e201a00102f527d3592089226febec58764437c6410a07e3cea0430
```

| Requirement | Mutant | Result |
| --- | --- | --- |
| V7 | predecessor native stored as container native | **KILLED** |
| V8 | constant route-authority account | **KILLED** |
| V9 named | omit runtime-generation comparison | **KILLED** |
| V10 named | remove receipt binder | **KILLED** |
| V9 required fence | omit active SeatBinding lifecycle check | **SURVIVED — F-8187-V11** |
| V10 lost ack | bind only on first `Applied::Updated` call | **SURVIVED — F-8187-V12** |

Named correction score: **4/4 killed**. Expanded required-fence score:
**4/6 killed**. The verdict is FAIL because the two survivors are expressly
required behavior, not optional hardening.

## Required correction before re-verification

1. Extend the committed generic-v1 test to retire the exact topology
   SeatBinding and prove refusal; rerun the active-lifecycle mutant.
2. Add a committed deterministic fault seam or equivalent store/daemon test for
   the interval after receipt recording and before binding. Prove same-key
   replay binds the original receipt, then rerun the bind-only-on-updated
   mutant.
3. Preserve all currently green behavior and rerun the same focused, schema,
   generated-contract and inherited-baseline commands.
