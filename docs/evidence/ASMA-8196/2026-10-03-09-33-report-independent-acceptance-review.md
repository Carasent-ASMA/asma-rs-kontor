# ASMA-8196 independent acceptance review

> **Date:** 2026-10-03 09:33 CEST
> **Status:** 🟢 Approved — accept-with-findings
> **Author:** Codex AUD seat (independent reviewer; not an implementation or verification author)
> **Category:** report
> **Scope:** Source candidate `940138c5578b67c1590587a0d29c10390a58847a` on `feat/ASMA-8196-roster-occupancy-read-port`, based on `f95e206563bca88b6871f48528623441e5e1a231`; evidence head `f77c32501c92aecad1ee10180edbfd3158cd06fb`
> **Summary:** Independent source-only acceptance review of the ASMA-8196 roster and occupancy read port. The behavioral contract and the corrective roster-persona rework are accepted; one non-behavioral rustfmt finding remains and is preserved below.

---

## When to Load

**Load this document when:**

- deciding whether source candidate `940138c5` satisfies the ASMA-8196 read-port acceptance contract;
- reviewing the preserved predecessor-persona rejection and its correction;
- distinguishing source acceptance from deployment, integration, permission, admission, or live-runtime evidence.

**Do NOT load for:** claiming deployment, merge, admission, permission, Jira movement, an external gate, or native prompt readback.

---

## Verdict

`accept-with-findings`

The source behavior is accepted. The epic membership fence, current and retired occupancy projection, generation-specific frozen persona identity, truthful null cases, frozen-launch-input semantics, read purity, and prompt-byte exclusion are present and covered by focused passing regressions.

The one finding is P2/non-blocking for behavior: `cargo fmt --all -- --check` reports two formatting deltas in the new claimant regression. The review authorization excludes source edits, so this record does not repair them or relabel the candidate as rustfmt-clean.

No P0 or P1 finding remains in the reviewed source scope.

## Independence basis

- This review was performed by the Codex AUD seat against the exact candidate and evidence commits. I did not author any candidate source commit, the rework, or the verification report.
- The candidate commit trailers identify Claude Opus 5 as implementation co-author. The supplied review routing identifies the verifier as Cursor. The current auditor is a different provider and role from both.
- The acceptance conclusion comes from an independent diff and control-flow review, independent hash and patch-id checks, and fresh focused test execution. Prior mutation claims were assessed as evidence rather than presented as mutations rerun by this seat.

## Candidate identity and scope

| Item | Observed value |
|---|---|
| Branch | `feat/ASMA-8196-roster-occupancy-read-port` |
| Base | `f95e206563bca88b6871f48528623441e5e1a231` |
| Port commits | `0612a839b7ef1379fa6c8dd4412646196166dd87`, `c56b80eba5879ee7f6a45afc43a1c2e3ed4d6cd5` |
| Corrective source head | `940138c5578b67c1590587a0d29c10390a58847a` |
| Re-verification evidence commit | `f77c32501c92aecad1ee10180edbfd3158cd06fb` |
| Review checkout | `/Users/igor/carasent/asma-modules/.worktrees/asma-8196/asma-rs-kontor` |

The evidence commit is a descendant of the corrective source head and changes only `docs/evidence/ASMA-8196/2026-10-02-22-06-report-roster-persona-reverification.md`. The source under test at evidence head is therefore the exact `940138c5` source.

Stable patch IDs independently match the preserved originals:

| Port | Preserved original | Stable patch ID |
|---|---|---|
| `0612a839` | `1be7cf01` | `b5b8ed032e1b4e06746fcbb3be4074e93a1a6108` |
| `c56b80eb` | `073d46ad` | `92e9b95b5cd509e32d37b935fcec069262b9fbb6` |

## Acceptance contract

| Contract clause | Finding | Evidence |
|---|---|---|
| Exact epic membership fence | Pass | `epic_hosted_seat_occupancies` calls `epic_control_seat_binding` before reading history/current occupancy. That helper finds the named epic's control plane and accepts only a binding held by it; foreign epic and foreign seat cases return 404 in the focused regression. |
| Current versus retired occupancy | Pass | History reads `ORDER BY retired_at ASC, rowid ASC`; history entries are emitted as `retired`, followed by the current seat as `current`, with monotonically positional occupancy generations. |
| Generation-specific frozen persona identity | Pass | `hosted_seat_occupancy_dto` calls `get_hosted_seat_role_persona(project_id, seat_binding_id, occupancy_generation)` and exposes only role, digest, delivery, generation, and freeze time. Retired and current occupancies cannot inherit one another's projected persona. |
| Truthful null for legacy adoption | Pass | The promotion regression proves the adopted first occupancy retains `null` after a later launched successor freezes its own persona. No backfill is inferred. |
| Truthful null for a current generation with no persona | Pass | `epic_core_team_dto` obtains the current occupancy generation from `hosted_topology_seat_occupancy_generation`, then performs the exact-generation persona lookup. The claim-over-launched regression proves the claimant is current, the roster and chain both report `null`, and the retired predecessor retains its digest. |
| Frozen launch input distinct from native prompt readback | Pass | `CoreTeamSeatPersonaDto` is separate from `CoreTeamNativeSeatDto`; delivery remains `create_only_no_readback`. The promotion regression independently reads the successor system prompt and initial handoff and proves they are distinct. No native-applied-prompt attestation is inferred. |
| Zero effects from GETs | Pass | Both routes are GETs composed only from store reads and cursor projection. The regression captures an unrelated cursor before the reads, repeats the roster read byte-for-byte, verifies no cursor advance, and verifies no runtime call. |
| No prompt bytes in the DTO | Pass | Neither Rust DTO nor the committed OpenAPI schema has a `prompt` property. Focused assertions reject a middle fragment of the persona in either response and require digest-only projection. |

## Preserved rejection and corrective finding

The first verification finding is preserved: the roster GET could attach a predecessor persona to a claimant native. The pre-existing projector introduced at `f06623f4cfd9dd8dcdd7d9725afeb5dcace6dc97` called `latest_hosted_seat_role_persona`, which answers the highest occupancy generation that has a persona row. A claimant opens a new occupancy without recording a persona, so that query returns the retired predecessor's row while the roster reports the claimant's native.

Commit `940138c5` closes that defect at its source. The roster now:

1. reads the current occupancy generation from the hosted seat using the store's `1 + COUNT(history)` definition;
2. reads the persona for that exact generation;
3. returns `null` when the current generation has no persona.

The new regression covers the previously missing order: launch generation one with a persona, supersede it with a claim that records none, then require roster/chain agreement on current `null` while retaining the retired generation-one persona. The fresh focused run passed.

`latest_hosted_seat_role_persona` now has no executable caller in `crates/`: its only non-definition occurrence is the explanatory comment at the corrected projector.

The rework did not change the occupancy route or API surface. The four API blobs are identical at `c56b80eb` and `940138c5`:

| File | Git blob |
|---|---|
| `crates/kontor-api/contract/openapi.json` | `067cf292b889ffca293ca2f909bc018725b41506` |
| `crates/kontor-api/src/applications.rs` | `d05678665252adb518a7c3ead4ed4b54538e9fd2` |
| `crates/kontor-api/src/lib.rs` | `322362bfb43a53192f107e22137e5ade87a442b6` |
| `crates/kontor-api/src/openapi.rs` | `508095dfd0ccb12a324e9557fc7ab89e6e58bce9` |

Function-body hashes also match at both commits:

| Function | SHA-256 |
|---|---|
| `epic_hosted_seat_occupancies` | `6da4f88183af0f155556f215ab52eb1abb7b5d9093b366a202abdd6467e4ba17` |
| `hosted_seat_occupancy_dto` | `f16babd0b78485fd62e776a170adbe19402bab561a6111fc10d73f43b667d56a` |
| `epic_control_seat_binding` | `450bdc1ac1336e00041df05eb30d88818f4fbdea8f80343f7c4215f63f2ee2d0` |

## Evidence quality

The committed re-verification report is present at evidence commit `f77c3250` and has SHA-256 `a99d9cf2249c9dab86b3a08050a7e02bdd1dd82388bb6c9e70f4fa61e40d0feb`.

The evidence chain is adequate and candid:

- The producer's underlying persona repair records four mutants killed out of four: missing successor role prompt, wrong successor occupancy generation, missing seat projection, and missing initial role prompt.
- The independent source re-verifier rebuilt and killed the two successor mutants (`MUT-8196-3` and `MUT-8196-4`) and restored the exact source before its passing rerun.
- The read-port commit records five killed mutants, including the initially surviving GET-write mutant whose test was strengthened with an unrelated pre-read cursor.
- The roster-persona re-verifier independently reintroduced the predecessor fallback, observed the claimant regression fail, restored the source, and reran the real source successfully.
- The recorded restore hashes match this checkout exactly: `applications.rs` is `3ccba358352695bc520b1f78383cb3c9c55ead483fdc6b0293784f7eac2c8fb1`; `loopback_api.rs` is `cc6507565e3b8b3842c4125a2aeb02e85db5056acb6fe12de905031bbac2efd5`.
- The report records the unrelated earlier parallel 409 in `legacy_message_proof_duplicate_split_across_pages_is_terminal_without_delivery`, states it was not reproduced in the serial/full 484-test run, and claims no candidate change for it. This review does not convert that unrelated flake into candidate evidence.

The mutation procedures were not repeated here because the authorization permits no source edits and the committed reports already preserve the mutate/kill/restore evidence. This seat instead confirmed the restored hashes and freshly ran the real focused regressions.

## Finding and dissent

### F-1 — P2: corrective regression is not rustfmt-clean

`cargo fmt --all -- --check` exits non-zero and proposes formatting-only changes in `crates/kontor-daemon/tests/loopback_api.rs` around the exact-generation store lookup and the claimant native-id assertion (current lines 65034 and 65054).

This does not change the accepted behavior: the test compiles and passes, `git diff --check` passes, and no functional discrepancy was found. It does prevent a clean formatting-gate claim. Source repair is outside this review's authorization, so the finding is preserved rather than fixed.

The earlier behavioral rejection remains part of the record and is not rewritten as though it never occurred. It is closed only for source candidate `940138c5` by the correction and the passing focused regression above.

## Exact checks run

All commands ran in `/Users/igor/carasent/asma-modules/.worktrees/asma-8196/asma-rs-kontor`.

```text
git rev-parse HEAD
git rev-parse --abbrev-ref HEAD
git merge-base --is-ancestor 940138c5578b67c1590587a0d29c10390a58847a HEAD
git merge-base --is-ancestor f77c32501c92aecad1ee10180edbfd3158cd06fb HEAD
```

Result: evidence head `f77c32501c92aecad1ee10180edbfd3158cd06fb`, expected branch, and both ancestry checks passed.

```text
git show 0612a839 --pretty=format: | git patch-id --stable
git show 1be7cf01 --pretty=format: | git patch-id --stable
git show c56b80eb --pretty=format: | git patch-id --stable
git show 073d46ad --pretty=format: | git patch-id --stable
```

Result: both port/original pairs produced equal stable patch IDs, as recorded above.

```text
git diff --unified=0 c56b80eb..940138c5 -- crates/kontor-daemon/src/applications.rs
git rev-parse <commit>:<each-api-path>
git show <commit>:crates/kontor-daemon/src/applications.rs | <brace-balanced-function-extraction> | shasum -a 256
rg -n 'latest_hosted_seat_role_persona' crates --glob '*.rs'
```

Result: the rework changes only the roster persona lookup inside `epic_core_team_dto`; the API blobs and occupancy functions match; there is no executable caller of the old latest-persona query.

```text
jq '.components.schemas.CoreTeamSeatPersonaDto, .components.schemas.HostedSeatOccupancyDto, .components.schemas.CoreTeamSeatDto' crates/kontor-api/contract/openapi.json
shasum -a 256 crates/kontor-daemon/src/applications.rs crates/kontor-daemon/tests/loopback_api.rs docs/evidence/ASMA-8196/2026-10-02-22-06-report-roster-persona-reverification.md
```

Result: prompt bytes are absent from the DTO schemas, and all restore/evidence hashes match the committed report.

```text
cargo test -p kontor-api --test openapi_contract
```

Result: PASS — 3 passed, 0 failed.

```text
cargo test -p kontor-daemon --test loopback_api -- --test-threads=1 --exact a_claim_superseding_a_launched_occupancy_reports_a_null_roster_persona
```

Result: PASS — 1 passed, 0 failed, 484 filtered out.

```text
cargo test -p kontor-daemon --test loopback_api -- --test-threads=1 --exact a_promotion_creates_one_epic_and_hands_the_work_to_its_lsa
```

Result: PASS — 1 passed, 0 failed, 484 filtered out.

```text
cargo fmt --all -- --check
```

Result: FAIL — formatting-only deltas described in F-1.

```text
git diff --check f95e206563bca88b6871f48528623441e5e1a231..940138c5578b67c1590587a0d29c10390a58847a
git diff --quiet 940138c5578b67c1590587a0d29c10390a58847a..f77c32501c92aecad1ee10180edbfd3158cd06fb -- crates/kontor-api crates/kontor-daemon
```

Result: PASS — no whitespace errors in the source diff and no source change between corrective source head and evidence head.

## Boundaries

- No source file was edited.
- No existing evidence file was modified.
- No deployment, runtime, topology, admission, permission, integration, or native-system-prompt-readback claim is made.
- No push, PR, merge, Jira mutation, or external gate record was performed.
- This verdict accepts only the reviewed local source candidate and preserves F-1 for the delivery owner.
