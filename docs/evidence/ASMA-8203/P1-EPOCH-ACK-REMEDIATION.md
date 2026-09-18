# ASMA-8203 audit seq2 P1 — epoch mappings could be lost by a failed write

Audit sequence 2 durably REJECTED, routed to high-implementation at workflow
revision 8. This remediates the sole P1.

## The defect

`ApiState::persist_drained_epochs` read the adapter's new epoch mappings with a
**take**:

```rust
let allocated = adapter.drain_new_timeline_epochs();   // EpochRegistry::drain -> mem::take
…
self.with_store(|store| store.persist_timeline_epochs(…, &allocated, at))?;
```

Handing the pairs over and their becoming durable are two different events, and
the take collapsed them into one. If the commit failed, the pairs were already
gone from `undrained` while the numbers stayed live in the registry's `by_raw`.
`EpochRegistry::resolve` then served those numbers from cache on every later
read, so they were never offered for persistence again — and the realm went on
addressing positions by numbers that exist in no durable record.

The next process would allocate from 1 again and hand the same numbers to
different raw epochs. That is exactly the drift migration `0100` exists to stop,
reintroduced through `0100`'s own write path, and it is the same class of failure
that made OG-061's preserved tuples unsettleable.

Provenance: `drain_new_timeline_epochs` with `mem::take` came from the durable
epoch-continuity work. The OG-061 commit `d3a7f46c` did not introduce it, but it
extracted the shared `persist_drained_epochs` helper and added a second caller
(`refresh_timeline_epoch_durably`), which widened the exposure to the recovery
path as well.

## The correction — transactional peek/ack

`drain_new_timeline_epochs` is replaced by a two-step contract on
`RuntimeAdapter`:

- `pending_timeline_epochs()` — a **peek**. Reading the list never discharges the
  obligation it reports, so a caller whose commit failed still finds the pairs
  on the next pass.
- `ack_timeline_epochs(persisted)` — the only thing that may clear a pending
  mapping, and it clears **exactly** the pairs named. Anything allocated while
  the caller was committing stays pending, in allocation order. Acknowledging a
  pair twice, or one never held, is a no-op: persistence is idempotent on both
  sides, so a retry after an ambiguous failure is not punished.

`ApiState::persist_pending_epochs` (renamed from `persist_drained_epochs`, since
nothing is drained any more) now runs read → commit → acknowledge, in that order.
A failure returns before the acknowledgement, leaving the pairs exactly where
they were.

`EpochRegistry::drain` becomes `pending()` (clone) and `ack()` (`retain`, which
preserves order). `by_raw` is untouched by an acknowledgement: durability and
"this process still uses the mapping" are different facts.

The failure direction is now the safe one. A crash between commit and
acknowledgement leaves pairs pending that are in fact durable, and the next
persist rewrites them — harmless, because the insert is
`ON CONFLICT (runtime_kind, host, raw_epoch) DO NOTHING`.

## Regressions

**`a_failed_epoch_persist_leaves_the_mapping_pending_until_it_lands`**
(`crates/kontor-daemon/tests/loopback_api.rs`) — the write is made to fail for
real, not simulated. `runtime_timeline_epochs` carries
`UNIQUE (runtime_kind, host, kontor_epoch)` while the insert's conflict target is
the primary key, so a decoy row claiming the same Kontor number under a different
raw epoch is a genuine constraint violation inside the transaction. The test:

1. reads the timeline once to learn the allocated number;
2. deletes that row, inserts the decoy, and makes the adapter forget the mapping;
3. asserts the next read is **refused** and the pair is **still pending**, with
   nothing written for it;
4. removes the decoy and asserts an ordinary retry serves, the mapping is durable,
   and only then is pending empty.

**`epoch_registry_tests`** (`crates/kontor-runtime-paseo/src/adapter.rs`) — three
unit tests directly on the production registry: the peek is repeatable and only
an ack clears it; an ack keeps what it was not told about, in allocation order,
and is idempotent for repeated or unknown pairs; a re-resolved raw epoch is never
queued twice.

## Mutation

| # | Mutation | Result |
|---|---|---|
| M1 | acknowledge **before** the commit — the original `mem::take` semantics | **killed**: `a mapping whose write failed is still pending: []` |
| M2 | `EpochRegistry::ack` clears wholesale instead of removing exactly what it was told | **killed**: later allocations lost — `left: []`, `right: [("raw-b", 2), ("raw-c", 3)]` |
| M3 | `pending_timeline_epochs` takes instead of peeking | **killed**: same pending-empty failure as M1 |

M1 and M3 reproduce the reported P1 exactly, from the two different places it
could be reintroduced.

## Carried in the same change: observation recovers the epoch too

Additive live evidence, from two newly launched scope runs whose session epochs
had never been re-read under the current numbering:

| Ticket | Agent run | Runtime binding | Native session |
|---|---|---|---|
| ASMA-8200 | `01a0b033-78bd-7691-90e0-a8b8a85fafab` | `01a0b033-78bd-7691-90e0-a8cdc874a086` | `038fc029-3d94-4508-96de-3bc11d9a34ca` |
| ASMA-8196 | `01a0b032-fe68-70c2-8cd7-e5c65207640e` | `01a0b032-fe68-70c2-8cd7-e5d2368c1f56` | `bf8b7ea4-3f20-4754-a7c9-a2f03a5bdbce` |

Both completed tiny current turns; Kontor could not derive a proof, and direct
reads refused every probed epoch with `timeline_refetch_required`. Settlement had
already learned to answer that signal — the read path *observation* uses had not,
so the one surface an operator is told to fall back to was the one that could not
recover.

The recovery is therefore lifted out of the daemon into
`ApiState::history_recovering_epoch_once`, and both callers share it: one bounded
tail epoch refresh, persisted through the failure-safe barrier above, then one
retry of the same anchored question. `Services::proof_scan_page` now delegates to
it instead of carrying its own copy, so there is one recovery in the realm rather
than two that can drift.

A second refusal is returned as the observer's own typed 409 via
`unobservable_epoch`. The code is kept — a client branches on it — but the advice
is replaced: `/timeline`'s "read the session timeline again from the runtime" is
right for a streaming consumer that can restart its own read, and circular for an
observer that has already re-read the epoch at the tail and been refused again.
What is left is observing afresh with no resume cursor.

`/timeline` and the SSE stream deliberately keep the raw signal and are unchanged.
Caller-supplied epochs are never accepted, and the recovery never origin-walks —
it reads one tail entry for the epoch alone.

### Regressions

- `observing_a_never_read_session_recovers_its_epoch_once` — the live shape. The
  runtime refuses every anchored read until the epoch is re-read; observation
  must answer once, report the same turn it would have reported anyway, and leave
  the mapping durable with nothing outstanding.
- `an_observation_whose_epoch_write_fails_keeps_it_pending` — the audit P1 on this
  path: the decoy row makes the recovery's own write fail, and the tuple is not
  reported while the mapping stays pending, until the obstacle is removed.
- `observing_a_current_turn_fails_closed_on_every_unreadable_shape` — already
  covered the second-refusal 409; strengthened to assert the actionable advice,
  which is what makes the typed restatement killable.

### Mutation

| # | Mutation | Result |
|---|---|---|
| M4 | observation reads through `history_with_durable_epochs`, with no recovery | **killed** by the never-read regression |
| M5 | `unobservable_epoch` passes the refetch signal through untouched | **killed**: the refusal reverts to "read the session timeline again from the runtime" — the circular advice |

## Also carried: the bounded turn-observe scan and its issuance ledger

The hang described below was real, and it is fixed here rather than deferred.

Observation with no resume cursor was an *origin* read, and Paseo answers those
by walking `start_cursor` backwards until nothing older remains — a request per
page of transcript. ASMA-8200 and ASMA-8196 were newly launched scope seats with
tiny completed turns that could not be observed at all, because the cost was the
transcript rather than the turn.

It is now seeded from one canonical tail read. `RuntimeAdapter::tail_window`
returns at most `max_pages` pages of the newest content, in ascending order,
within one epoch, and promises nothing about what precedes them. Paseo implements
it as the same backwards stepping the origin walk did, stopped at a budget
instead of at the beginning of the session, keeping the non-advancing guard and
epoch agreement verbatim. `next` is always `None`: the window ends at the tail.
Observation reads it through the same one-refresh-one-retry recovery and the same
persist-before-expose barrier as any other read, with `TAIL_WINDOW_PAGES = 8`.

### What the issuance ledger does and does not prove

**Corrected.** An earlier revision of this document claimed the ledger made a
*stronger* statement than counting occurrences, and that a repeat of an id was
still refused. Both were overstated, and a later audit was right to reject on it.
The precise statements are:

- `runtime_message_issuances` proves **one issuance**: this realm minted the id
  once and issued it to exactly one binding. That is a primary-key lookup.
- The bounded window proves **one occurrence inside the window**.
- Neither proves **one canonical occurrence**. A runtime that echoes a message
  produces two mentions of a singly-issued id, and a window bounded to the tail
  sees only the newer — the earlier is excluded by not being looked at.

Counting the rest is the scan the bound exists to remove, so the missing half is
supplied in v105 by pinning the delivery position rather than by scanning; see
*Occurrence pinning* below. Until that pin is checked, the ledger alone is not a
uniqueness proof and must not be described as one.

Nothing is backfilled. A message issued before the ledger existed has no row and
never will, and observation refuses it with an action rather than guessing: send
the seat one new correlation message. The resume-cursor path is untouched, so
`prefix_occurrences` and the duplicate-straddle audit test keep their existing
mechanism and assertions.

### Migration slot — needs an integrator decision

This branch takes **`0104`** (`0104_runtime_message_issuances.sql`,
`SCHEMA_VERSION = 104`). **ASMA-8193 also owns a pending `0104`.** Whichever
lands second must renumber to `0105`: `MIGRATIONS` is position-based and
`MIGRATIONS.len() == SCHEMA_VERSION` is a `const` assert, so a duplicate slot is
a compile failure rather than a silent merge conflict. Taking the next free slot
at rebase time is the convention; it is recorded here so the integrator does not
have to rediscover the collision.

### Deployment readback changes

The next deployment carrying this moves the live realm from schema **103 to
104**. The previous receipt's check was "must remain 103"; it becomes "must reach
104", and the pre-deploy database backup stops being precaution and becomes
actual migration cover.

### Regressions

- `observing_a_long_never_read_transcript_costs_the_window_not_the_session` —
  200 turns in front of a two-event turn at the tail, epochs forgotten. Asserts
  the exact message and terminal positions **and** that observing cost ≤ 2
  session reads, so the number cannot scale with the transcript.
- `observing_refuses_a_current_turn_older_than_its_window` — the bound fails
  closed, with an action naming a new message rather than a longer read.
- `observing_refuses_a_tail_window_that_does_not_advance` — a window whose
  positions repeat names no turn.
- `observing_trusts_only_a_message_this_realm_issued_to_this_session` — a real
  send records the issuance against this binding before delivery; an id this
  realm never issued is refused 404; an id issued to another session is refused
  409.
- `a_message_issuance_is_unique_per_id_and_recognises_its_own_replay` — the
  ledger itself: first issuance records, an identical replay is recognised, a
  different session claiming the id is refused, and an unissued id is absent
  rather than invented.
- `observing_a_current_turn_pages_a_long_session_and_resumes_from_its_anchor`
  kept, with its obsolete starvation assertion replaced: at `limit=1` the old
  origin-seeded read returned 503 having never reached the turn; the tail-seeded
  read answers and still names the newest turn.

### Mutation

| # | Mutation | Result |
|---|---|---|
| N1 | observation no longer seeds from the tail | **killed**: 9 session reads instead of 1 |
| N2 | the issuing path omits the ledger write | **killed**: a real send no longer issues the id it delivers |
| N3 | the store's uniqueness comparison is dropped | **killed**: a foreign session's claim returned `Replayed` |
| N4 | observation drops the binding-match guard | **killed**: an id issued to another session was accepted 200 |

## Rework: the bounded window admitted a forward gap

High-verification-report `artifact-asma-8203-high-verification-report-12c804d0`
revision 2 (`01a0b311-c3f3-7d03-b3f5-f992ad1ee430`) confirmed the epoch peek/ack,
the issuance ledger, the 103→104 migration, the bounding, the refresh, the typed
refusal and the no-origin-walk seams, and **rejected** on one finding: the new
bounded tail path accepted a forward sequence gap `N → N+2`, reducing the
continuity invariant `HistoryReader` had held.

The finding is correct. `HistoryReader::accept_page` required each position to
follow the last one *adjacently*, so a skipped sequence was a break. The tail
window replaced that with `sequence <= last.sequence` — monotonic, which `N+2`
satisfies. A window missing an event therefore read as one continuous stretch of
session, and the missing event is precisely the kind this scan reasons about:
another addressed message, which would mean the turn being described is not the
newest one, or the response that decides terminality.

### The correction, at both seams

`observe_from_tail` now requires `sequence == last.sequence + 1` between adjacent
events. The **first** sequence stays unconstrained deliberately: a tail window
begins wherever the budget reached, so demanding it start at 1 would be demanding
the origin walk back. Only the joins are constrained.

`PaseoAdapter::tail_window` had the same hole one level down. Stepping backwards
a page at a time, it proved only that each step made *progress* — that the older
page ended before the cursor it was fetched behind. Progress is not a join: a
runtime skipping a sequence between two pages satisfied it while handing back a
window with a hole spliced into the middle. Pages are now merged only where
`older_end + 1 == window_start`.

The repeat and skip refusals are one fence now, so
`the runtime's tail window does not advance` became
`the runtime's tail window is not continuous`.

### Regressions

- `observing_refuses_a_forward_gap_anywhere_in_the_bounded_window` —
  parameterised over `from_end ∈ {1, 3}` at `limit=3`, so one gap falls inside
  the page the window starts from and the other exactly on the join behind it.
- `observing_accepts_a_window_that_begins_mid_session` — the half that would make
  the fence useless if it were wrong: twelve turns first, asserting the turn
  under test does not start at sequence 1, and the window still observes cleanly.
- `a_tail_window_refuses_pages_that_do_not_join` and
  `a_tail_window_joins_contiguous_pages` (Paseo contract) — a journal missing
  sequence 5 read two entries per page, and its contiguous control.
- New fake hook `skip_next_tail_event(from_end)` places the hole at a chosen
  offset; `observing_refuses_a_tail_window_that_does_not_advance` retargeted to
  the merged rule.

### Mutation

| # | Seam | Mutation | Result |
|---|---|---|---|
| C1 | observation | adjacency reverted to ascending | **killed** — returned **200** with `message_sequence: 11, response_sequence: 14`: a settleable tuple spanning the hole, which is the rejected defect exactly |
| C2 | Paseo merge | join reverted to backward progress only | **killed** — returned a window containing `1,2,3,4,6,7`, spliced straight through the missing 5 |

No schema change: migration `0104` and every schema-104 behaviour are untouched,
as are the seams the verifier had already passed.

## Post-deployment: the reads were bounded in requests, not in time

Approved gap `operational-gap-kontor-proof-reader-hang-20260918` rev 3
(approval `01a0b520-7e65-7ef2-a0ea-dd6fc0a4098b`). After `af0d1896` + schema 104
deployed with identities preserved, settle retries for ASMA-8205 and ASMA-8202
hung past a minute and committed nothing, timeline reads for the ASMA-8203 and
ASMA-8193 verifier runs hung past thirty seconds, and startup logged stale-binding
restore refusals with 315 bindings `needs_review`.

### Root cause: missing bounded *operation* timeout

Not pagination, not transport, not stale-binding handling.
`PaseoTransport::request` already wraps every request in
`tokio::time::timeout(timeout_seconds)`, defaulting to **30 seconds**, so each
individual request behaved correctly. What had no bound was the **read**.

A derived read issues a page at a time under a page budget, and against a session
the runtime will not answer for, every page costs that full 30 seconds. A
settlement proof is bounded at 64 window pages plus a refresh, a retry and 64
trailing pages — bounded in requests, unbounded in the only unit a caller feels.
The arithmetic matches the report exactly: a timeline read hanging past 30s is
*one* request timing out; a settle hanging past 60s is a chain of them, ended by
the caller rather than by the realm. The 315 `needs_review` bindings are the
supply of unanswerable sessions, not the mechanism.

### The correction

`ApiState` carries a `derived_read_deadline`, plumbed through `DaemonConfig`
exactly like `evidence_window_seconds`, defaulting to **20 seconds** —
deliberately below one request's own deadline, because a healthy bounded read
costs milliseconds and anything approaching this is a runtime that is not
answering. It wraps every runtime-history seam:

- `history_recovering_epoch_once`
- `tail_window_recovering_epoch_once`
- `ensure_raised_here`

The third was not in the report and is the same defect: it paged in an unbounded
`loop` with no page budget at all. Elapsing is reported as the runtime being
unreachable and writes nothing; a settlement's own mapping then names it an
incomplete proof scan and tells the operator to settle the same turn again later.

A session this process holds no snapshot for is still refused by one registry
lookup, before any request — which is what matters when 315 bindings did not
restore.

## Occurrence pinning: one issuance is not one occurrence

Approved audit `artifact-asma-8203-high-audit-report-af0d1896` revision
`01a0b533-07c5-75c2-93e8-e2c89ff9f278` rejected on the gap named above: the
no-cursor observer checked occurrences only inside its eight-page window, so an
earlier duplicate outside it could be excluded while the current one was
accepted.

Counting the rest would reinstate the scan, so the question is asked from the
other side. `MessageAck` already reports the position a message landed at, which
is a fact about *this delivery* rather than about the transcript. Migration
**0105** adds nullable `delivered_epoch` / `delivered_sequence` to the v104
ledger; both issuing paths — `send_message` and `deliver_follow_up` — record the
acknowledged position after delivery; and the bounded observation requires the
occurrence it reports to sit exactly there.

"Is this the only occurrence?" needs a scan. "Is this the occurrence Kontor
delivered?" is a lookup and a comparison. An earlier duplicate cannot be reported
because it is not at the recorded position, and a later echo cannot either.

The position is first-write-wins: re-recording the same one is the retry of a
delivery whose acknowledgement was lost and is accepted; a *different* position
for an id already delivered is the runtime saying the message landed twice, and
is refused rather than overwritten. A row that never gains a position — a
delivery whose acknowledgement was lost — is refused with the same action as a
pre-ledger id, because the realm cannot say which occurrence was meant.

Preserved: no origin walk, bounded latency, issuance uniqueness, forward-gap
continuity, and every identity.

### Schema and deployment expectation

This head carries **104 → 105**. Schema 104 is already live, so 0105 is an
ordinary forward migration, but the redeploy readback changes from "must reach
104" to "must reach 105", and the pre-deploy backup is migration cover. The
migration-slot collision with ASMA-8193 now covers **both** `0104` and `0105`:
whichever lane lands second renumbers, and the `const` assert makes a duplicate
slot a compile failure rather than a silent conflict.

## Suite evidence — combined hotfix cut

`/tmp/p1-occurrence-suites.log`, one `===ALL-DONE===` marker, `--no-fail-fast`
throughout.

| Phase | Result |
|---|---|
| `kontor-store`, `kontor-runtime`, `kontor-api`, `kontor-runtime-paseo`, `kontor-mcp` | 49 blocks (incl. 5 doc-test blocks), **1057 passed, 0 failures** |
| `kontor-daemon --test loopback_api` | **369 passed, 8 failed, 1 ignored** |
| `kontor-tests-contract` | 9 blocks, **all ok**, 0 failures |

Counts reconcile: loopback 378 total = 374 at the continuity cut + the 4 new
regressions in this one (`a_derived_read_is_bounded_when_the_runtime_never_answers`,
`a_settlement_against_a_silent_runtime_is_bounded_and_writes_nothing`,
`an_unattested_session_is_refused_without_reaching_the_runtime`,
`observing_refuses_a_duplicate_whose_delivery_is_outside_the_window`). All five
new or extended tests passed, including the store's
`a_message_issuance_is_unique_per_id_and_recognises_its_own_replay`.

The eight loopback failures are the unchanged known baseline set, verified
identical at base `86ba6065`: six fail with code `unavailable` / "the configured
native Jira connector could not answer"; `a_session_key_must_be_a_stable_client_message_id`
fails on the `MessageId::derive` change from master `9d5a81b5` (#222); and
`replaying_a_partial_admission_delivers_its_durable_follow_up` fails with
`revision_conflict` from a hardcoded expected revision. None touch the derived-read
deadline, the delivery-position pin, the bounded window, the continuity fence or
the issuance ledger.

`--no-fail-fast` is used deliberately: an earlier run without it let a
contention flake in one store binary stop cargo scheduling, silently truncating
four targets and every doc-test, and "37 blocks with one known flake" would have
read as a clean phase.

## Suite evidence — continuity cut (`6ae38e6d`)

`/tmp/p1-continuity-suites.log`, one `===ALL-DONE===` marker, plus
`/tmp/p1-continuity-phase1-rerun.log` for phase 1.

| Phase | Result |
|---|---|
| `kontor-store`, `kontor-runtime`, `kontor-api`, `kontor-runtime-paseo` | 45 blocks (41 targets + 4 doc-tests), **985 passed, 0 failures** |
| `kontor-daemon --test loopback_api` | **365 passed, 8 failed, 1 ignored** |
| `kontor-tests-contract` | 9 blocks, **all ok**, 0 failures |

Counts reconcile: loopback 374 = 372 + the 2 new gap regressions; phase 1 985 =
983 + the 2 new Paseo contract tests. The eight loopback failures are the
unchanged known baseline set.

### Why phase 1 was run twice — and why the first run could not be accepted

The first pass reported `a_concurrent_first_open_initializes_exactly_one_realm`
failing with `DatabaseBusy` / "database is locked", while two unrelated lanes
were running full workspace suites on the same machine. That test passes in
isolation in ~1.9s against 173.8s in-suite, so the failure itself is lock
contention rather than a logic fault.

The important part is what the failure *did*. The run had no `--no-fail-fast`, so
cargo stopped scheduling after that binary: 37 blocks instead of 45, no doc-tests
at all, 928 passed instead of 983. Four targets never executed. A contention
flake anywhere in the package set silently truncates everything after it, and
reading "37 blocks, one known flake" as a clean phase would have been accepting
coverage that was never run.

Phase 1 was therefore re-run with `--no-fail-fast`, which reported the full 45
blocks green. `--no-fail-fast` is the right default for this environment for the
same reason.

## Suite evidence — previous cut (`12c804d0`)

`/tmp/p1-ledger-suites.log`, exit 0, one `===ALL-DONE===` marker, run on the
committed tree.

| Phase | Result |
|---|---|
| `kontor-store`, `kontor-runtime`, `kontor-api`, `kontor-runtime-paseo` | 45 result blocks, **all ok**, 0 failures (6 ignored in one block, pre-existing) |
| `kontor-daemon --test loopback_api` | **363 passed, 8 failed, 1 ignored** |
| `kontor-tests-contract` | 9 result blocks, **all ok**, 0 failures |

The eight loopback failures are the known baseline set, unchanged by this
remediation and previously verified identical at base `86ba6065`: six fail with
code `unavailable` / "the configured native Jira connector could not answer";
`a_session_key_must_be_a_stable_client_message_id` fails on the
`MessageId::derive` change from master `9d5a81b5` (#222); and
`replaying_a_partial_admission_delivers_its_durable_follow_up` fails with
`revision_conflict` from a hardcoded expected revision. None touch the epoch
barrier, the tail window or the issuance ledger.

Counts reconcile: 372 loopback tests = 368 before this cut + the 4 new loopback
regressions.

All eight tests this remediation depends on passed in that run:

```text
a_failed_epoch_persist_leaves_the_mapping_pending_until_it_lands ... ok
a_message_issuance_is_unique_per_id_and_recognises_its_own_replay ... ok
an_observation_whose_epoch_write_fails_keeps_it_pending ... ok
observing_a_never_read_session_recovers_its_epoch_once ... ok
observing_a_long_never_read_transcript_costs_the_window_not_the_session ... ok
observing_refuses_a_current_turn_older_than_its_window ... ok
observing_refuses_a_tail_window_that_does_not_advance ... ok
observing_trusts_only_a_message_this_realm_issued_to_this_session ... ok
```

`cargo fmt --check` clean per crate for all five touched crates. Formatting was
run per-crate rather than `--all`, because the workspace is not fmt-clean at
every commit and `--all` would widen the diff beyond the owned files.

One environmental note, recorded because it appeared in an earlier run of the
same suite and could otherwise be mistaken for a regression:
`a_concurrent_first_open_initializes_exactly_one_realm` failed twice with
`DatabaseBusy` / "database is locked" while an unrelated full workspace suite was
running concurrently in another worktree, taking 163–168s for a block that takes
~2s alone. It passes in isolation and passed in this final run. It is SQLite lock
contention, not a logic fault.

## Mutation — all fourteen killed

Three separate defects are closed in this remediation, and each was proven by
mutation on the exact seam it fixes.

| # | Seam | Mutation | Result |
|---|---|---|---|
| M1 | epoch barrier | acknowledge before the commit (the original `mem::take` semantics) | killed — `a mapping whose write failed is still pending: []` |
| M2 | epoch registry | `ack` clears wholesale instead of exactly what it was told | killed — `left: []`, `right: [("raw-b", 2), ("raw-c", 3)]` |
| M3 | epoch barrier | `pending_timeline_epochs` takes instead of peeking | killed — same pending-empty failure |
| M4 | observe recovery | observation reads without the epoch recovery | killed by the never-read regression |
| M5 | observe refusal | the typed restatement is dropped | killed — reverts to the circular "read the session timeline again" |
| N1 | tail bound | observation no longer seeds from the tail | killed — 9 session reads instead of 1 |
| N2 | issuance ledger | the issuing path omits the ledger write | killed — a real send no longer issues the id it delivers |
| N3 | issuance ledger | the store's uniqueness comparison is dropped | killed — a foreign session's claim returned `Replayed` |
| N4 | issuance ledger | observation drops the binding-match guard | killed — an id issued to another session was accepted 200 |
| C1 | window continuity | adjacency reverted to ascending | killed — 200 with `message_sequence: 11, response_sequence: 14`, a tuple spanning the hole |
| C2 | Paseo page merge | join reverted to backward progress only | killed — window contained `1,2,3,4,6,7` |
| D1 | derived-read deadline | the operation timeout removed | **killed by hanging** — the same test returns in ~1s with the deadline and never returns without it; observed still running at 20s, 30s and 40s before being terminated |
| E1 | occurrence pinning | the delivered-position comparison removed | killed — returned **200** with `message_sequence: 33`, accepting the tail echo as a settleable tuple while the real delivery sat outside the window: the audited defect reproduced |
| E2 | delivery record | a contradictory delivery position accepted | killed — `an id already delivered may not claim a second position: Ok(())` |

D1 is the one mutant whose kill is a hang rather than an assertion, and that is
the honest shape of it: without an operation deadline there is no failure to
report, only a call that does not come back. It was run in the background,
observed not to return, and terminated.

E1 is non-vacuous in the specific way the audit required: the echoed id has a
recorded delivery position, so the test reaches the position comparison rather
than the missing-position branch, and the assertion is on the exact rule text so
it cannot pass by refusing for some other reason.

M3 survived its first attempt: the durability assertion was vacuous because an
earlier ordinary read in the same fixture had already persisted the mapping. The
test was rewritten to forget the mappings first and to assert on the *undrained*
side in the failure path, where no later page can discharge the obligation. That
is recorded rather than quietly fixed, because a mutant that survives once is the
only evidence that the assertion was not proving what it claimed.

## Noted, out of scope

The store's constraint refusal surfaces to the caller as `revision_conflict`
("re-read the aggregate and retry with the revision it reports"), which reads
oddly for a timeline-epoch write. The P1 is about the pending list, and the
label is a separate question; the regression asserts only that a page addressed
by a non-durable number is not served.

The fake adapter's `ack_timeline_epochs` is exercised end-to-end by the loopback
regression, but with a single pending entry, so a wholesale-clear mutant would
survive there. The equivalent mutant is killed on the production `EpochRegistry`
(M2), which is what ships.
