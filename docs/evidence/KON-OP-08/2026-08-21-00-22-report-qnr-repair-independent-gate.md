# KON-OP-08 — QNR-P1 repair independent gate

> **Date:** 2026-08-21 00:22 CEST
> **Status:** 🔴 Rejected — bounded QNR-P1 repair
> **Author:** Inspector · KON-OP-08
> **Category:** report
> **Scope:** ASMA-7877 task revision 2, TeamRun `01a0195b-7280-7500-81cf-c28023f8cbf8`, QNR-P1 control-surface and current-master replay-convergence repair
> **Summary:** Independently rejects commits `6f884a0e` and successor `d310cdc2`. The successor fixes bounded refetch and replay paths but accepts cardinality-incompatible topology repins and retains two mutation-test evidence gaps. This verdict is not a merge approval.

---

## When to Load

**Load this document when:**

- deciding why the QNR-P1 replay/control-surface repair at `6f884a0e` did not
  pass its independent live gate;
- reconciling PR 64's bounded corrective evidence or the cursor-384 QNR
  readback;
- correcting and re-reviewing the live cursor-free history invariant.

**Do NOT load for:** whole-PR merge authorization, replacement QNR topology,
GATE-RE-PORT admission, ASMA-7929 launch, or Jira-state inference.

---

## Bounded verdict

Commit `6f884a0e552272d05583c3db6f4edcf414a44f40` is **rejected as a
complete bounded QNR-P1 repair**. The six-file corrective delta from
`4e3e8fd774c04c6bd95bcd99f66b143df615b142` is internally coherent and its
static proof is green, but the handed-off live endpoint reproduced the
original blocking call shape after recovery; see QNR-FND-001.

The candidate closes three independently reproduced defects in its committed
test environment:

1. cursor-free Paseo history treats the tail as discovery and walks backward
   through runtime-issued `startCursor`/`before` pages to the epoch origin;
2. an acknowledged message is followed by exact-binding inspection and the
   shared AgentRun/TeamRun observation reducer; and
3. an unrouted legacy task TPM is retired in place on materialization replay,
   while an already-hosted task seat remains routable and messaging cannot
   create an identity.

Those results do not outweigh the failed live acceptance invariant. This is
deliberately a bounded verdict. It does not erase or supersede the task-level
PR verdict in
`2026-08-20-22-44-report-independent-inspector-rereview.md`; see OQ-001.

## Blocking finding

### QNR-FND-001 — cursor-free read again returns 409 on the recovered live endpoint

**Severity:** P1 / gate-blocking.

**Required invariant:** for an exact active AgentRun and preserved hosted
binding, `session-timeline-get` without `--after` must deterministically return
HTTP 200 and a runtime-issued epoch-origin anchor. If an epoch or retention
window changes during the backward walk, the cursor-free operation must restart
from a fresh tail within a bounded policy rather than return the same
`timeline_refetch_required` instruction that the caller already followed.

**Independent reproduction:** after the supervised daemon logged `startup
reconciliation finished` and `kontor is serving` at `2026-08-20T22:22:10Z`,
the exact ASMA-7679 AgentRun
`01a01ce6-5991-7921-95af-c2b8b5c0fb2f` behaved as follows:

1. continuation from the earlier epoch-6 cursor returned 409
   `timeline_refetch_required`; that stale-cursor refusal is valid by itself;
2. the instructed cursor-free refresh with `--limit 1` returned the same 409;
3. a second bounded cursor-free refresh with `--limit 1` again returned the
   same 409 while `127.0.0.1:7717` remained listening.

The independent `task-get` immediately afterward succeeded at authoritative
cursor 389, confirming that the control plane was serving rather than merely
unreachable. This is the original QNR-P1 failure mode: a caller that obeys the
refetch instruction still cannot obtain a stream anchor.

**Smallest corrective test/invariant:** add a Paseo contract case such as
`timeline_cursor_free_read_restarts_from_a_fresh_tail_after_runtime_refetch`
that scripts invalidation during `tail → before`, then supplies a stable fresh
tail and requires an epoch-origin HTTP-200 result. The implementation should
bound whole-walk retries, discard every cursor from the invalidated attempt and
restart at `tail`; it must not weaken epoch/gap checks or turn a cursor-bound
`after` request into a cursor-free read. The exact native reason for the live
invalidation remains to be captured rather than inferred.

## Immutable inspection identity

| Item | Value |
| --- | --- |
| Realm | `01a00649-9ee6-73e0-ba1b-6a6c35cfd065` |
| QNR project | `01a0064a-e056-7603-9968-ef64fdaacb75` |
| QNR epic | `01a019c0-eee7-72a1-a8a7-7fff1ddce8f3` / `ASMA-7675` |
| OP-08 task | `01a0074f-672e-79a3-9876-d0e1bf585d4e`, revision 2 |
| OP-08 TeamRun | `01a0195b-7280-7500-81cf-c28023f8cbf8` |
| Builder AgentRun | `01a01eb0-453d-7c21-ac60-bee1d8cf8d73` |
| Builder turn | `01a0213a-f1a4-77a1-b29e-05b509f0ec5d`, ordinal 3 |
| Inspector AgentRun | `01a01ead-7837-7bf1-b63b-cd596c9b0d97` |
| Base | `6868a1414bc44adc0eb0813ced8943c8f41734b2` |
| Head | `6f884a0e552272d05583c3db6f4edcf414a44f40` |
| Pull request | [PR 64](https://github.com/Carasent-ASMA/asma-rs-kontor/pull/64) |

Local `HEAD` and `origin/master` independently resolved to the head and base
above. Their merge base is the same base, with no base-only commits.

## Independent code inspection

### Timeline origin and continuation

`PaseoAdapter::history` still resolves every caller cursor against the exact
binding and preserves canonical projection and raw-epoch continuity. On a
cursor-free request it now:

- performs the only available cursor-free `tail` read;
- follows `hasOlder` using each page's runtime-issued `startCursor` and
  `before` direction;
- refuses a missing cursor, epoch change, empty/non-progressing older page or
  sequence break; and
- returns the oldest page with a normal `after` continuation.

`HistoryReader` was not weakened. Cursor-bound requests still use `after` and
the same strict no-gap/no-overlap validator.

### Message observation reduction

The session route now preflights both `SendMessage` and `Inspect` before the
first native effect. It resumes the exact issued binding, sends or reconciles
the stable message id, inspects that same binding after acknowledgement, and
persists the runtime's actual observation through the existing shared reducer.
The reducer advances AgentRun and TeamRun together; it does not infer
`running` from the acknowledgement.

The supported runtime families remain compatible: every runtime family in
this build that advertises `SendMessage` also advertises `Inspect`. A runtime
without the evidence surface is refused before delivery.

### Legacy task TPM route

Operational semantics place persistent LSA/TPM control roles at the epic
control plane and admit TSW delivery seats through a TeamRun. The repair
therefore takes the authorized non-materialization branch for old logical-only
task TPM rows: a replay retains the same SeatBinding as evidence but releases
it so it no longer publishes itself active. It does not mint an AgentRun,
hosted session, workspace or successor identity.

The topology-message guard now distinguishes TeamRun delivery seats from a
task-bound seat that already has a hosted route. A hosted task seat can be
messaged under its existing identity; a missing hosted route remains a typed
placement refusal.

## Independent tests and gates

| Gate | Independent result |
| --- | --- |
| `timeline_cursor_free_read_walks_back_to_the_epoch_origin` | PASS |
| `a_message_resume_reduces_the_run_and_team_run_back_to_running` | PASS |
| `ticket_materialization_retires_an_unrouted_legacy_tpm_without_creating_identity` | PASS |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `cargo test --workspace` | PASS; rerun outside the restricted network namespace after the sandbox correctly refused a loopback bind |
| `cargo deny check` | PASS; advisories, bans, licenses and sources all `ok` |
| `git diff --check` | PASS |

The workspace run independently included Paseo contract `138/138`, daemon
loopback `195/195`, the CLI/MCP/contract suites, repository migrations and
doc-tests. Live-daemon-only tests that declare external prerequisites remained
explicitly ignored as designed.

### Mutation proof

Each mutant was applied alone and restored before the next one:

| Mutant | Narrow suite | Result |
| --- | --- | --- |
| Disable the cursor-free backward walk | Paseo origin regression | **KILLED** by `TimelineRefetchRequired { reason: SequenceGap }` |
| Skip post-message observation persistence | message projection regression | **KILLED**: AgentRun remained `waiting_input` instead of `running` |
| Skip legacy task-TPM release | task TPM regression | **KILLED**: binding remained `active` instead of `retired` |

Every mutated source file was restored to the committed blob. `git diff
--exit-code` and `git diff --check` are clean. The one untracked KON-MVP-18
fixture directory created by the workspace test was removed; all directories
present at the inspection baseline remain untouched.

## Hosted checks

Direct `gh pr checks 64` readback at the exact head returned all four required
jobs terminal and passing:

- [Console gates — job 96595412100](https://github.com/Carasent-ASMA/asma-rs-kontor/actions/runs/32421862731/job/96595412100);
- [Console gates — job 96595425286](https://github.com/Carasent-ASMA/asma-rs-kontor/actions/runs/32421866536/job/96595425286);
- [Rust workspace gates — job 96595411851](https://github.com/Carasent-ASMA/asma-rs-kontor/actions/runs/32421862731/job/96595411851); and
- [Rust workspace gates — job 96595425053](https://github.com/Carasent-ASMA/asma-rs-kontor/actions/runs/32421866536/job/96595425053).

## Live exact-identity evidence

An independent cursor-free observer call for ASMA-7679 AgentRun
`01a01ce6-5991-7921-95af-c2b8b5c0fb2f` succeeded on the preserved binding
`01a01eac-7c50-7561-b2ea-c4cd8bed0bf8`. It returned HTTP 200 with:

```text
anchor: 01a01eac-7c50-7561-b2ea-c4cd8bed0bf8:6:1
epoch: 6
first sequence: 1
next: 01a01eac-7c50-7561-b2ea-c4cd8bed0bf8:6:1
```

That earlier result proved the candidate can return an origin under one live
history shape. The builder's durable turn records the same origin followed by
sequences 2 and 3, the cursor-381 through cursor-384 materialization/message
receipts, unchanged native identities and four-TeamRun cardinality.

It was not stable enough to pass the gate. After the listener recovered, the
independent continuation was correctly refused as stale, then two independent
cursor-free refreshes both returned 409 `timeline_refetch_required`; see
QNR-FND-001. No direct database read, QNR mutation or replacement identity was
used to fill that failure.

## Preservation

This inspection changed no tracked candidate code and made no QNR, AgentsRoom,
Jira, topology, gate, TeamRun, AgentRun, SeatBinding or native-session
mutation. It created no replacement run or workspace. It did not arm
GATE-RE-PORT and did not launch ASMA-7929.

The exact cursor-384 and cursor-387 QNR cardinality, GATE-RE-PORT zero-TeamRun
state and ASMA-7929 absence are builder-turn evidence. The inspector
independently verified one successful origin read and the later recurrent 409
on the same AgentRun, without inferring later topology state.

## Provisional post-rejection patch inspection

After OQ-004 had been recorded, the inspector took the least expansive option
its ledger permits: treat the complete working-tree patch against local merge
head `d783ccddc7390bb82363415ded32a2b4b9475c0f` as a provisional snapshot and
stop if its content digest changed. The initial binary patch SHA-256 was
`47c6b184adc32504d29e0006840755d9668802cd22433f95cd461e84d961f328`
(`851` insertions, `71` deletions across nine tracked files).

### Positive verification

- At provisional digest `47c6b184`, focused timeline-refetch, compatible
  topology-repin, epic projection,
  pre-v47 naming and MCP root-kind baselines passed;
- at that digest, `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets -- -D warnings` and Paseo contract
  `137/137` passed;
- the complete `kontor-store` package suite passed while its changed slice
  remained unchanged; and
- after the unrelated working-tree drift described below, daemon loopback
  passed `195/195` on the later `82eb0472` content.

Three one-at-a-time production mutants were killed and restored:

1. disabling the fresh-tail retry failed the new timeline test with
   `TimelineRefetchRequired { reason: EpochChanged }`;
2. skipping the node-stamp repin failed the store assertion that every node
   cites the target immutable revision; and
3. reading the project default instead of the epic pin failed the loopback
   assertion that the embedded projection reports version 2.

### PROV-FND-001 — topology repin accepts a target whose cardinality the existing epic violates

**Severity:** blocking correctness defect in provisional digest `47c6b184`.

`repin_mini_project_nodes_in` validates each existing node's kind, parent,
native-container capability and seat-host capability, then rewrites every node
stamp. It never checks the target specification's `NodeCardinality` minimum or
maximum against the existing children. A one-at-a-time fixture mutation changed
the target ECP cardinality to exactly two while the epic held one ECP. The
supposedly compatible repin succeeded and the focused test stayed green.

The smallest correction is to validate the relevant existing epic tree against
the target's cardinality rules before the update and to add both minimum and
maximum incompatibility regressions. The validation must occur in the same
transaction and must preserve the current per-epic/project-root boundary.

### PROV-FND-002 — daemon-replacement transport safeguard has no behavioral test

**Severity:** blocking evidence gap for the claimed MCP recovery repair.

Deleting `.pool_max_idle_per_host(0)` from `HttpTransport::new` left the full
MCP suite green: `44` library tests, `2` binary tests and `5` seat tests all
passed. The change specifically claims to recover a long-lived MCP process from
a poisoned pooled connection after the supervised daemon is replaced, but no
test performs that lifecycle. Add a loopback transport test that reuses one
`HttpTransport`, replaces or drops/rebinds the server between calls, and proves
the second call reaches the recovered daemon. Re-run the deletion mutant.

### PROV-FND-003 — progressed-seat outage guard still has the prior survivor

**Severity:** blocking evidence gap retained from RR-FND-003 on the later
`82eb0472` content.

No test request combines `unavailable_provider` with a run that has progressed
beyond launch. Disabling the durable lifecycle/desired/observed guard again left
`an_admin_retires_an_exact_never_dispatched_provider_blocked_seat` green. The
mutant was restored. A regression must progress that same exact run first and
prove replacement is refused before any runtime retirement.

### Snapshot stop

All inspector mutants were restored. The patch returned to digest `47c6b184`
before the long package verification. During that verification the mutable
working tree changed independently: `loopback_api.rs` grew from `128` changed
lines to `136`, and the complete patch digest became
`82eb0472ce15e48c4c5c0808f63b9d689732c58202f233fe359934fce4bf4e3f`.
The inspector therefore issued no verdict for either provisional digest and did
not inspect or test the later delta as though it belonged to the frozen one.

## Immutable successor verdict — `d310cdc2`

The builder subsequently committed and pushed exact head
`d310cdc2be77110b7b88b9fec38d8804aceba05d` on base
`0c58e72fddf505238a6d4c884935b130f3f1e7d0`. Its binary diff from merge head
`d783ccddc7390bb82363415ded32a2b4b9475c0f` hashes to exact SHA-256
`82eb0472ce15e48c4c5c0808f63b9d689732c58202f233fe359934fce4bf4e3f`,
proving that it is the later provisional content above. The tracked worktree and
branch/remote readback are clean and identical.

`d310cdc2` is **rejected**. The inspector independently reran all three surviving
diagnostics after the commit became immutable:

1. changing the target ECP cardinality to exactly two while one ECP exists
   still lets the repin succeed and the focused store test remain green;
2. deleting `.pool_max_idle_per_host(0)` still leaves the complete MCP suite
   green (`44 + 2 + 5` tests); and
3. disabling the progressed-run launch-only guard still leaves
   `an_admin_retires_an_exact_never_dispatched_provider_blocked_seat` green.

Each diagnostic edit was restored immediately. `git diff --exit-code`,
`git diff --check`, branch/remote identity and the untracked-evidence baseline
were re-read afterward. PROV-FND-001 through PROV-FND-003 are therefore
blocking findings against the immutable successor, not merely a moving patch.

Hosted readback for this exact head is terminal and green on all four required
jobs:

- [Console gates — job 96618357533](https://github.com/Carasent-ASMA/asma-rs-kontor/actions/runs/32429585621/job/96618357533), PASS in 34 seconds;
- [Console gates — job 96618360107](https://github.com/Carasent-ASMA/asma-rs-kontor/actions/runs/32429586572/job/96618360107), PASS in 30 seconds;
- [Rust workspace gates — job 96618357846](https://github.com/Carasent-ASMA/asma-rs-kontor/actions/runs/32429585621/job/96618357846), PASS in 18 minutes 13 seconds; and
- [Rust workspace gates — job 96618360230](https://github.com/Carasent-ASMA/asma-rs-kontor/actions/runs/32429586572/job/96618360230), PASS in 11 minutes 31 seconds.

Green required automation does not resolve PROV-FND-001 through
PROV-FND-003, which the required workflows do not exercise.

## Open-question ledger

### OQ-001 — bounded repair verdict versus the existing whole-PR gate

- **Subject:** whether this QNR-P1 repair verdict is intended to supersede or
  replace the earlier task-level PR 64 rejection.
- **Attached record:** this report;
  `2026-08-20-22-44-report-independent-inspector-rereview.md`; PR 64 head
  `6f884a0e552272d05583c3db6f4edcf414a44f40`.
- **Why ambiguous:** the handoff explicitly names a bounded QNR-P1 repair and
  asks for its independent gate. The six-file corrective delta closes that
  repair, but it does not touch the earlier report's Jira convergence, retained
  OP-08 scope or provider-outage negative-test findings. No explicit
  supersession or disposition record was supplied.
- **Options observed:** retain this as a bounded repair rejection while the
  whole-PR gate remains unchanged; authorize a complete whole-PR re-review that
  disposes every earlier finding; or correct and independently re-review the
  earlier findings.
- **Disposition:** unresolved. This report records only the bounded rejection
  and makes no change to the earlier whole-PR verdict.

### OQ-002 — continuation readback during supervised daemon startup

- **Subject:** independent live readback of sequences 2 and 3 after the repaired
  epoch-origin anchor.
- **Attached record:** this report; builder turn
  `01a0213a-f1a4-77a1-b29e-05b509f0ec5d`, ordinal 3; ASMA-7679 AgentRun and
  binding above.
- **Why ambiguous:** the cursor-free origin request succeeded, then the
  supervised daemon restarted before the first continuation attempt.
- **Options observed:** wait for startup reconciliation to finish and repeat the
  exact read-only continuation; or treat listener unavailability as a failed
  deployment check.
- **Disposition:** resolved. The listener recovered and logged serving. The old
  continuation was then correctly stale, but the instructed cursor-free
  refresh failed twice. That result is now QNR-FND-001 rather than an
  availability ambiguity.

### OQ-003 — independently identifying the installed daemon artifact

- **Subject:** whether the installed daemon's bytes can be independently tied
  to candidate commit `6f884a0e`.
- **Attached record:** this report; builder turn
  `01a0213a-f1a4-77a1-b29e-05b509f0ec5d`; installed
  `/Users/igor/.local/bin/kontor-daemon`; workspace `target/release/kontor-daemon`.
- **Why ambiguous:** the handoff records deployment of the candidate and the
  installed daemon timestamp is `2026-08-21T00:18:00+0200`, but `--version`
  reports only `0.2.0`. Its SHA-256
  (`ef695266fc5be566834efbc3dc47bcfa641b432c9df9a2afc7334d07c0cbae1e`)
  differs from the extant workspace release artifact
  (`7411fb2d941a7ddd4af03f3866ceaae64a4911b666187d745c39cbca1b75f46e`),
  so byte identity cannot be established from those two files alone.
- **Options observed:** expose the source commit/build digest through a
  read-only daemon surface; retain the exact deployed artifact plus build
  receipt; or rebuild/install the pinned head and compare the deployed digest.
- **Disposition:** unresolved. The live failure is attributed to the endpoint
  handed off as the candidate deployment, not to an independently proven source
  commit inside the binary.

### OQ-004 — immutable identity of the post-rejection corrective artifact

- **Subject:** which immutable artifact the new builder-finished handoff asks
  this inspector to review.
- **Attached record:** this report; branch
  `feat/ASMA-7877-kontor-operational-control-surfaces`; prior rejected head
  `6f884a0e552272d05583c3db6f4edcf414a44f40`; current local merge head
  `d783ccddc7390bb82363415ded32a2b4b9475c0f` on current master
  `0c58e72fddf505238a6d4c884935b130f3f1e7d0`; the nine tracked modified paths
  reported by `git status` through the fourth builder-finished handoff signal.
- **Why ambiguous:** the handoff says the builder finished and recorded its
  artifacts, but supplies no new base, candidate commit, patch digest or turn
  receipt. The local branch now contains a current-master merge while its
  remote still resolves to the previously rejected commit. All nine corrective
  files are staged, and `applications.rs`, `loopback_api.rs` and
  `operational_topology.rs` also carry additional unstaged edits, so the index
  and working tree represent distinct mutable candidates:
  `kontor-daemon/src/applications.rs`, `kontor-daemon/src/lib.rs`,
  `kontor-daemon/tests/loopback_api.rs`, `kontor-mcp/src/client.rs`,
  `kontor-mcp/src/registry.rs`,
  `kontor-runtime-paseo/src/adapter.rs`,
  `kontor-runtime-paseo/tests/contract.rs`, `kontor-store/src/repository.rs`
  and `kontor-store/tests/operational_topology.rs`. Ownership and finality
  cannot be inferred from worktree presence.
- **Options observed:** commit and hand off an exact base/head; publish and
  freeze an exact patch digest with its owning turn receipt; or identify these
  edits as non-candidate concurrent work and provide the actual artifact.
- **Disposition:** resolved after the fifth builder-finished handoff signal. A
  read-only `run-get` for preserved builder
  AgentRun `01a01eb0-453d-7c21-ac60-bee1d8cf8d73` could not recover the missing
  identity because the handed-off loopback endpoint returned typed
  `unavailable` with `dispatched: false`; a later listener check found no
  process bound to `127.0.0.1:7717`. A provisional digest was then explicitly
  frozen under this ledger, but it changed from `47c6b184` to `82eb0472` during
  verification. The builder then committed and pushed the exact later content
  as `d310cdc2`; its commit diff reproduces digest `82eb0472`. The independent
  successor verdict is the rejection above.

## Recording checkpoint

This document is the independent **rejected** verdict for the bounded QNR-P1
repair at `6f884a0e` and immutable replay-convergence successor `d310cdc2`. A
cursor-389 `task-get` returned
`current_phase: implementation` and `gates: {}`. The pinned `code` profile
declares `code-review-gate` only in the `code-review` phase and currently
reports it `not_ready`. Advancing task lifecycle was outside this inspection,
and OQ-001 leaves the bounded verdict's relationship to the whole-task gate
ambiguous. No typed whole-task gate was submitted, no evaluator account or
authority was inferred, and the PR remains unapproved by this inspection.
