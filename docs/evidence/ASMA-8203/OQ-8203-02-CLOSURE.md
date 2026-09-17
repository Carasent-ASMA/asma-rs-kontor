Artifact: `high-change`

# OQ-8203-02 closure: a settlement tuple that spans two turns

Date: 2026-09-17
Task: Jira `ASMA-8203` / Kontor `01a0ac9d-a96d-75b2-9803-169d4f2e531f`
Branch: `feat/ASMA-8203-attach-runtime-message-proof-to-delivery-turns`
Companion records: `HIGH-SCOPE-RECORD.md`, `HIGH-CHANGE-RECORD.md`, `OG-058-CLOSURE.md`

## Scope correction

This was previously recorded as OPEN and out of scope. That reading was wrong.
The approved high-scope acceptance requires missing, forged, stale, **mismatched**,
reused and non-terminal proof to be rejected, and requires the *exact current
turn tuple*. An older message paired with a newer turn's terminal response is a
mismatched tuple. It is in scope and is now corrected.

## The defect

`Services::prove_current_turn` proved three things independently — that the
claimed id sat at the claimed message position, that the claimed response
position held a provider response, and that the response was the last
non-state/non-log event — and never proved the two positions belonged to *one*
turn. A caller holding an older message id could pair it with the current turn's
terminal response and settle:

```
older turn   message 1:7   response 1:8
newest turn  message 1:9   response 1:10
settled with {message_id: <older>, message_position: 1:7, response_position: 1:10}
  -> 200 OK, applied "created", turn_ordinal 1, dispatched follow-up to `builder`
```

Every existing condition held: the scan was exhausted, `message_matches == 1`
(the id really is at 1:7), `response_matches == 1` (1:10 really is a provider
response) and `last_turn_position == response_position` (1:10 really is the
tail). The user message at 1:9 — a turn boundary sitting *inside* the claimed
window — was never examined.

## The correction

`crates/kontor-daemon/src/applications.rs`, `prove_current_turn` — the smallest
authoritative boundary, since this is the settlement authority itself. The scan
now also counts any event strictly after `message_position` and no later than
`response_position` whose subject is `EventSubject::Message(_)`:

```rust
if event.position.epoch == message_position.epoch
    && event.position.sequence > message_position.sequence
    && event.position.sequence <= response_position.sequence
    && matches!(event.subject, EventSubject::Message(_))
{
    newer_messages_inside += 1;
}
```

Tool calls, permission events and the response itself carry no message subject
and remain ordinary turn content; only another *addressed* message is a turn
boundary. The check is deliberately reported **last**, after the existing count
checks, for two reasons: it then fires only when every other half of the tuple
is genuine and the window itself is the sole remaining fault, and it leaves the
existing cases refusing under their own rule so they keep killing M2 and M3. It
carries its own sentence, because it wants a different correction from the
caller — re-read the current turn, not the positions:

> a newer runtime message lies inside the claimed window, so it spans more than
> the current turn

## Regression

`settling_a_bounded_turn_refuses_a_forged_current_window` gains a fifth case,
`turn-forged-spanning-window`, reproducing the live shape exactly: the older
tuple's message half with the newest turn's response half. It asserts the new
rule, the two-runtime-call bound, and the same zero-write boundary as the other
four — no settled turn, no artifact evidence, no dispatch. The helper is now
parameterised by expected rule so each case pins its own.

## Mutation

| Mutant | Deliberate defect | Result |
| --- | --- | --- |
| M5 | replace the spanning check with `let _ = newer_messages_inside;` | **killed** — `left: 200, right: 409`, and the red body shows `applied: "created"`, `turn_ordinal: 1` and a dispatch to `builder`: the live hole, reproduced |
| M2 | re-run after the change — drop the message-subject check | still killed |
| M3 | re-run after the change — drop the terminality check | still killed |

M2 and M3 were re-run because this change edits the function they target; both
still fail closed, confirming the reorder did not mask them.

## Tests

`cargo test -p kontor-daemon --test loopback_api settling_a_bounded_turn` — 3/3.
`kontor-api`, `kontor-runtime`, `kontor-runtime-paseo`, `kontor-mcp`,
`kontor-tests-contract` — all pass. `cargo fmt --check` clean; `cargo clippy` on
every touched crate clean.

## Deployment — first attempt BLOCKED on a stale base, then resolved by rebase

### Attempt 1, on the `f78d041` base — refused, realm briefly down

The fix is daemon-side, so the daemon surface is the one required. It **cannot
be deployed from this branch**, and this was established the hard way.

The new daemon was built, backed up (`kontor-daemon.pre-asma-8203-20260917T034421Z`),
installed atomically (inode `492656608` → `494407467`, signature valid) and the
launchd job `com.asma.kontor.daemon` kickstarted. It refused to start:

```
ERROR kontor_daemon kontor could not start
  detail=the control-plane database could not be opened:
         database schema version 98 is newer than this binary understands (96)
```

This branch's highest migration is `0096_consultation_subject.sql`. The live
realm is at schema **98**; `0097`/`0098` arrived on other branches
(ASMA-8187 / ASMA-7869 / bounded control-plane recovery) that are already
deployed. A daemon built here is schema-older than the database it must open.

The previous binary was restored immediately by the same atomic method and the
realm is healthy: pid 44033, `live: true`, `schema_version: 98`,
`reconciliation: open`, `scheduling_open: true`, installed sha
`7bd9b0fb263470b48fc6fac329f0c990e6fe3776bff73713261859a3af1133db`.

**The realm was unavailable for roughly 32 seconds** (first refusal 03:44:29Z,
recovery logged 03:45:01Z) while launchd retried against the schema-incompatible
binary. No data was written by the failed binary — it refused before opening the
database. Callers in that window would have seen connection failures.

### Attempt 2, rebased onto current `origin/master` — deployed

The refusal was a stale-base integration fault, not a decision. The patch was
replayed onto the exact verified head:

```
origin/master expected  1e3f35af3de1086ddc5dead52ab94b1b28950d42
origin/master actual    1e3f35af3de1086ddc5dead52ab94b1b28950d42   (match; no drift)
```

`git rebase` onto that commit applied both ASMA-8203 commits with **zero
conflicts**, so no conflict resolution was required or performed. Equivalence was
required and proven, not assumed: the functional diff was normalized (blob
hashes and hunk line numbers stripped) before and after the replay and is
**byte-identical**, `sha256 04838a776733833820b95ad70b4abeda7ddbe54218e1718f7024d9b30c11c57a`
on both sides, 1874 lines.

Tests and mutations were re-run on the rebased head: the forged-window matrix
3/3, both delivery regressions green, M5 still killed (`left: 200, right: 409`),
G1 still killed, tree restored clean after each.

Rebuilt from the rebased head `26fd984` and deployed atomically:

```
kontor-daemon  built 24907f5260be0a1f45cbe4d0284428d1a077c2aee6e776a07c9ad715f45c6040
               inode 494407925 -> 494460725   signature valid
kontor-mcp     built f2d3367f8f115e2ab3fad56c3dc7002ea6086f42938371cbb1515449a1cf47dc
               byte-identical to installed — not redeployed
```

`launchctl kickstart -k gui/<uid>/com.asma.kontor.daemon`, then health polled to
open:

```
pid 63340   live: true   schema_version: 98
reconciliation: pending -> open      scheduling_open: true
log: startup scheduling barrier settled  barrier=Open
```

The deployed binary provably carries both fixes, and its predecessor provably
does not:

```
deployed  kontor-daemon : "a newer runtime message lies inside the claimed window…"  1 occurrence
predecessor             : same string                                                0 occurrences
deployed  kontor-daemon : "delivery_unconfirmed" present in the error-code table
```

### Surfaces not requiring redeploy

| Surface | Built alone | Installed | Action |
| --- | --- | --- | --- |
| `kontor-mcp` | `f2d3367f8f11…c47dc` | `f2d3367f8f11…c47dc` | byte-identical — none |
| `kontor` CLI | `02e4d8ecd512…77ac` | `14113ed2eac0…7157` | differs only by linking the OG-058 `kontor-api` delta; does not reference `ApiErrorCode`; not required by this change — left alone |
| `kontor-daemon` | `24907f5260be…6040` (rebased head) | `24907f5260be…6040` | **deployed** |

Note: building several packages in one `cargo build` invocation changes
`kontor-mcp`'s bytes through feature unification even when its sources are
identical. Each binary above was built in its own invocation, which is how the
installed artefacts were produced.

## Bounded out-of-scope items

**OQ-8203-03 (SIGKILL mechanism) — remains OPEN and out of scope.** Not
reproduced under either in-place mechanism; no fix is claimed. Bounded to:
capture a crash report or AMFI log line at the next occurrence. No code here
addresses it.

**OQ-8203-04 (Jira publication path makes the same "nothing was changed" claim)
— remains OPEN and out of scope.** Bounded to the Jira connector's error
mapping, behind its own ticket, and only once that suite is green again — six of
its tests fail at baseline for unrelated environmental reasons. No Jira code was
changed here.

## Gates

OG-058 and OG-059 remain **open**. No gate is advanced by this record.
Verification and audit remain required and unmet, and the deployment blocker
above must be resolved before any live-proof gate can be satisfied.

## Post-deploy verification on the live realm

**Health / reconciliation** — pid 63340, `live: true`, `schema_version: 98`,
`reconciliation: open`, `scheduling_open: true`, barrier `Open`.

**Identity preserved**, read back through the new daemon and byte-identical to
the pre-rebase reading:

```
task_id  01a0ac9d-a96d-75b2-9803-169d4f2e531f
state    in_progress | phase high-implementation | revision 2
binding  confirmed · ASMA-8203 · revision 2
         readback_hash 94f9809d5475f5a86174f3d174aecc743762d5b010e39f37f94366a2a7e62208
         confirmed_at  2026-09-16T23:59:51.926606Z
```

The rebase and redeploy changed nothing about the UUID↔key binding.

**Real observation → settlement**, attempted against the new daemon:

```json
{"code":"revision_conflict",
 "rule":"the exact settling seat is not freshly waiting after the named current turn"}
```

Unchanged and still correct. The implement seat `01a0acf9-8531-…` reads
`running` because it is this session composing this response, so no terminal
response event exists at any canonical position. `prove_current_turn` requires
`waiting_input` plus an exact terminal response, and it is the first gate
reached. **No implement receipt can exist until after this response is terminal**;
it must be produced by a post-turn control caller, exactly as OQ-8203-01
resolved. After the refusal `role_turns` still holds one row — the scope turn —
so the zero-write boundary holds on the live realm too.

## Pre-existing on master, deliberately not fixed here

Both were verified at clean `1e3f35af3de1086ddc5dead52ab94b1b28950d42` in a
detached worktree with its own `CARGO_TARGET_DIR`, so neither is caused by this
patch, and neither is ASMA-8203-owned:

1. **`cargo fmt --all --check` is dirty on master** — 5 diffs, in
   `crates/kontor-api/src/applications.rs` and three assertion sites in
   `crates/kontor-daemon/tests/loopback_api.rs`. The same 5 appear on the
   rebased head at shifted line numbers. Reformatting them would rewrite code
   this ticket does not own, so they are left alone. Every file this patch
   *does* touch is rustfmt-clean.
2. **`kontor-api --test openapi_contract` fails on master** —
   `openapi.json no longer matches what this crate serves; regenerate it with
   KONTOR_UPDATE_CONTRACT=1`. Regenerating would sweep master's unrelated
   contract drift into an ASMA-8203 commit, so the golden is left untouched.
   This patch adds no route and no DTO: `ApiErrorCode` is `value_type = String`
   in the schema, so the new `delivery_unconfirmed` variant does not appear in
   the document.


## Attempt 3 — master drifted mid-flight, and attempt 2 had regressed the realm

`origin/master` moved from `1e3f35a` to `d287de7` while attempt 2 was being
verified (`7eacdab` #225 *Fill admitted TeamRun slots owed a handoff*, then
`d287de7` #226). Per the fail-closed rule the drift was detected, work stopped,
and master was re-read rather than assumed.

Re-reading found something worse than a stale base. The daemon deployed in
attempt 2 was built from `1e3f35a` and therefore **did not contain #225**, while
the binary it replaced **did**:

```
deployed in attempt 2 (24907f52…)            "…no seat may be filled"  0 occurrences
its predecessor       (7bd9b0fb…)            "…no seat may be filled"  1 occurrence
```

Attempt 2 removed a master fix from the live realm. That is a regression I
introduced, not a pre-existing condition, and rolling back would not have
repaired it — the rollback target was the same stale lineage. The correct repair
was forward.

### Resolution

Re-rebased onto `d287de78bce4944d617f43d52d532f1c5d35bf8c` (re-verified
immediately before the operation; clean, zero conflicts). Master's two new
commits touch `crates/kontor-daemon/src/applications.rs` (+314) and
`crates/kontor-daemon/tests/loopback_api.rs` (+627) but **not**
`prove_current_turn` or the settlement path, so nothing ASMA-8203 owns needed
resolving.

Functional equivalence was re-proven across all three bases. Comparing the
normalized diff restricted to `crates/` and `tests/` — evidence prose is
excluded because it legitimately grew as the work proceeded:

```
f78d041 base : 9d58e0687c0a2d76fe90249d20297b4522598c0fda8fc25a7304ea37c9fecc6c
d287de7 base : 9d58e0687c0a2d76fe90249d20297b4522598c0fda8fc25a7304ea37c9fecc6c
```

Byte-identical. Focused tests 5/5 on the new head; M5 still killed
(`left: 200, right: 409`); tree clean after restore.

Rebuilt and deployed both surfaces atomically — `kontor-mcp` now genuinely
changed too, because master's #225/#226 edited `crates/kontor-mcp/src/registry.rs`:

```
kontor-daemon  434f0bc86fd6adbb24ed1391b5e9c74ad65705f13b7e490f73372dcacff75186  inode 494533353 -> 494548576
kontor-mcp     8d99a8e4ee4af4231145aa273deb102479fd440fb095c20f89877f266dace7dc  inode 494533361 -> 494548582
```

Post-deploy verification, pid 30844:

```
live: true   schema_version: 98   reconciliation: pending -> open   scheduling_open: true

deployed kontor-daemon contains:
  "startup reconciliation has not finished, so no seat may be filled"   1   (#225 restored)
  "a newer runtime message lies inside the claimed window…"             1   (ASMA-8203)
  "delivery_unconfirmed"                                               4   (OG-058)
```

Identity unchanged again — `confirmed · ASMA-8203 · revision 2`, readback_hash
`94f9809d…`, `confirmed_at 2026-09-16T23:59:51.926606Z`.

Settlement attempt against the final daemon returns the same correct refusal
(`the exact settling seat is not freshly waiting after the named current turn`),
and `role_turns` still holds only the scope row — zero writes.

### Live-realm impact, stated plainly

Three restarts of the shared realm were performed. One outage of roughly 32
seconds (attempt 1, schema-incompatible binary, rolled back). Attempt 2 left the
realm running without master's #225 for approximately 12 minutes before it was
detected and repaired by attempt 3. No data loss: attempt 1's binary refused
before opening the database, and attempts 2 and 3 both opened schema 98 normally.


## Full loopback suite, and an eighth pre-existing failure

The full `kontor-daemon --test loopback_api` run completed at **329 passed, 8
failed, 1 ignored** (1049s). The eighth failure is one the earlier targeted runs
never reached:

```
a_session_key_must_be_a_stable_client_message_id
  left: 200   right: 400
```

It asserts that a non-`MessageId` `Idempotency-Key` on
`POST /v1/sessions/{id}/messages` is refused `400 invalid_request`. It now
returns 200, because `message_identifier` derives an id instead of refusing:

```rust
MessageId::parse(key.as_str()).unwrap_or_else(|_| MessageId::derive(key.as_str()))
```

**This is not caused by this patch.** `crates/kontor-api/src/sessions.rs` is
touched here only by the OG-058 `after_delivery` change; `git diff` against the
base shows no edit to `message_identifier`. Verified the same way as the other
seven: a detached worktree at clean `d287de78bce4944d617f43d52d532f1c5d35bf8c`
with its own `CARGO_TARGET_DIR` fails identically, `left: 200, right: 400`, with
none of this branch's changes applied. Master changed the derive behaviour and
did not update the assertion.

Eight failures, eight verified pre-existing at the base commit. It is worth
saying explicitly that the earlier handoffs reported only focused and affected
suites; this failure was invisible to those and surfaced only on a full run.

## The observation surface (ASMA-8203 durable requirement)

`GET /v1/sessions/{agent_run_id}/turns/current`, served as `kontor_turn_observe`
at Observer tier. Reports the Kontor message id the runtime echoed and the two
canonical positions bounding the turn, with field names matching
`TurnRuntimeProofRequest` so relaying into a settlement is a copy.

Read-only by construction: same canonical history path `/timeline` uses, same
cursor, same `HistoryReader` validation. It writes nothing, attests nothing,
settles nothing. `kontor_turn_settle` is **unmodified** and remains the only
validator and only writer — the end-to-end regression asserts both that an
observation relayed verbatim is accepted and that a tampered one is still
refused.

### Mutation evidence — and two tests that were vacuous until it ran

| Mutant | Deliberate defect | First result | After strengthening |
| --- | --- | --- | --- |
| O1 | answer even when the seat is not freshly waiting | **survived** | killed |
| O2 | stop requiring the response to be the last canonical turn event | **survived** | killed |
| O3 | treat a repeated message id as an answer rather than divergence | killed | killed |
| O4 | answer from a partial scan instead of refusing at the page budget | **survived** | killed |

Three of the four initially survived, which means those tests proved nothing on
their own axes:

* O1 survived because the still-working case asserted only `409
  revision_conflict`, and a later guard refuses that shape too. Fixed by pinning
  the exact rule so the liveness check must be what refused.
* O2 survived because no test placed turn content *after* the response. Fixed by
  adding `observe_trailing_tool_call` to the fake — a tool call carries no
  message subject, so nothing reads it as a new turn, which is precisely why
  terminality needs its own check rather than falling out of the message scan.
* O4 survived because the pagination test stayed inside the 64-page budget.
  Fixed by reading the same long session at `limit=1`, far past the budget.

Recording this rather than only the final green: the first three tests looked
reasonable and were not.

### Coverage

Pagination (41 turns read at `limit=3`, newest turn correctly reported, anchor
resumes), page-budget exhaustion, epoch change (cursor naming an unknown epoch →
`timeline_refetch_required`), foreign cursor (refused before the runtime is
asked), missing id, duplicate id, unfinished turn, non-terminal response, and
forged / reused proof through the unchanged settler.


## Suite failure counts: what they actually were

Three full-suite runs reported wildly different failure counts (8, then 56, then
a crash). The differences were **not** code. Two environmental causes, both
mine to have noticed earlier:

1. **Concurrent load.** The 56-failure run overlapped release builds and three
   live-daemon restarts. Re-running the suspicious members individually on a
   quiet machine — `the_credential_file_is_owner_only`,
   `the_authority_tiers_are_enforced_per_route`,
   `the_contract_document_lists_every_application_route_and_no_unsafe_surface`,
   `the_model_catalog_advertises_every_route_used_by_operational_seats`,
   `the_registry_key_matches_what_the_fake_issues`,
   `settling_a_bounded_turn_refuses_a_forged_current_window` — all passed.
2. **The disk was full.** The next run aborted with
   `io error when listing tests: Os { code: 28, kind: StorageFull, message: "No
   space left on device" }`. The volume was at 100%, 560 MiB free of 926 GiB.
   This worktree's `target/` held 27 GB, of which 14 GB was
   `target/debug/incremental` — a regenerable compilation cache. Dropping it
   returned 18 GiB. Other worktrees' and other sessions' build directories were
   left alone.

No failure count from a contended or storage-starved run is reported here as a
property of the code, and the earlier "8 pre-existing" figure is withdrawn as
unreliable for the same reason — it came from a run that also overlapped release
builds. The authoritative numbers are from the clean run recorded below.


## Clean full-suite triage (authoritative run)

`/tmp/clean2.log`, from the restored ASMA-8203 tree with ~90 GiB free:
**340 passed, 8 failed, 1 ignored**, 1205s, zero storage errors.

All 8 were run again at clean `d287de78bce4944d617f43d52d532f1c5d35bf8c` in a
detached worktree with its own `CARGO_TARGET_DIR`: **0 passed, 8 failed** —
every one reproduces at the base with none of this branch applied.

| Failure | Attribution |
| --- | --- |
| 6 × Jira description/publication | environmental: each refuses with `"the configured native Jira connector could not answer"` (503 where 200 expected). They stand up a `wiremock::MockServer`; the connector is unreachable in this environment. Identical at base. |
| `replaying_a_partial_admission_delivers_its_durable_follow_up` | asserts a hardcoded `expected_task_revision: 1` against a task that has moved. Identical at base. |
| `a_session_key_must_be_a_stable_client_message_id` | **investigated as in-scope, proven not.** Master commit `9d5a81b5` (#222, ASMA-8190) deliberately changed `message_identifier` from refusing a non-`MessageId` key to `MessageId::derive`-ing one. That commit edited `loopback_api.rs` (+54 lines) but did not update this assertion, leaving it pinned to the old behaviour. Not in this branch (`git log d287de7..HEAD` does not contain it), and whether the derive or the assertion is right belongs to ASMA-8190. Not waived — attributed. |

Earlier reported counts of 8 and 56 came from runs that overlapped release
builds and live-daemon restarts, and one aborted on a full disk; none is used as
evidence here.

## MCP parity: four pins this change had to move deliberately

Adding `kontor_turn_observe` failed six assertions that exist precisely so a new
tool cannot appear unreviewed. Each was moved with its reason recorded:

* worker profile size 18 → **19** (`registry.rs`), plus an explicit
  `allows("kontor_turn_observe")`;
* worker served-at-operator 18 → **19**, and observer-visible reads 10 → **11**
  (`server.rs`) — an Observer-tier read widens what an observer *sees* without
  widening authority, which is the property that assertion holds;
* the tool's declared args now match the contract's `after` and `limit` query
  parameters;
* the tier allowlist gains `("kontor_turn_observe", CallerTier::Observer)`;
* the canary's registry count 178 → **179**, advertised 177 → **178**, and
  documented 179 → **180**.

The documented count moves by two against a registry of one because the
regenerated contract also documents master's `fill_team_run_seat` (#225), served
but never written into the document and with no MCP tool. That gap is master's;
it is named in the assertion rather than hidden by a matching number.
