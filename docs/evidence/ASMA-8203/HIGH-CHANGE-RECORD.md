Artifact: `high-change`

# ASMA-8203 high-change record: runtime message proof on delivery turns

Date: 2026-09-17
Task: Jira `ASMA-8203` / Kontor `01a0ac9d-a96d-75b2-9803-169d4f2e531f`
Epic: Jira `ASMA-8190` / Kontor `01a0abdd-b84e-7be2-a1f6-b9f34aae8c23`
Project: `01a0064a-e056-7603-9968-ef64fdaacb75`
Phase: `high-change`
Baseline: clean `f78d041`
Scope record: `docs/evidence/ASMA-8203/HIGH-SCOPE-RECORD.md`

## Outcome

> **Superseded in part.** This record covers the original ASMA-8203 scope —
> regression and mutation hardening of the existing delivery proof path — and
> its statements below are scoped to that work alone. The branch was
> subsequently expanded to close operational gap OG-058, which *did* change
> production code and did trigger build, deploy and live readback. See
> `OG-058-CLOSURE.md`. Read the two together; neither is complete alone.

For the ASMA-8203 proof-path scope: regression and mutation hardening only,
**no production code changed**. Within that scope the tree differs from
`f78d041` in two test files and this record:

```
 crates/kontor-daemon/tests/loopback_api.rs    | 215 +++++++++++++++++++++++++-
 crates/kontor-runtime-paseo/tests/contract.rs |  84 +++++++++-
 2 files changed, 295 insertions(+), 4 deletions(-)
```

All four targeted mutants M1–M4 were run one at a time against otherwise clean
production source, each was killed, and each was restored before the next. The
scope record's expected result — hardening with no production delta — holds.

One matter is **not** closed: a settlement shape outside the three the scope
record named is accepted by the deployed guard today. It is recorded as
OQ-8203-02 below with a reproduction, and no production change was made for it.

## What changed

### 1. Daemon loopback negative matrix (boundary item 1)

`crates/kontor-daemon/tests/loopback_api.rs` —
`settling_a_bounded_turn_refuses_a_forged_current_window`, with the shared
per-case helper `refuse_forged_current_window`.

Each case takes a tuple that would otherwise settle and changes exactly one
element, so a guard that stopped checking that element has nowhere to hide.
Every case asserts four things, not just the status:

* HTTP 409 and code `revision_conflict`;
* the stable rule string `the supplied message and terminal position are not
  the exact current runtime turn`;
* exactly two further runtime calls — one fresh inspect, one bounded history
  read — so the refusal costs no runtime effect beyond proving;
* zero writes: `list_settled_turns` empty (which covers the artifact evidence
  carried on a turn) and `list_turn_dispatches` empty.

| Case | Forgery | Tuple |
| --- | --- | --- |
| `turn-forged-identity` | a different valid message id | current positions, foreign `MessageId` |
| `turn-forged-message-position` | a wrong user-message position | true current id claimed one position early |
| `turn-forged-terminal-position` | a response no longer terminal | the previous tuple after a newer turn landed |
| `turn-forged-response-position` | a response position holding a non-message event | current message paired with the post-turn status event |

The test closes with a control: the genuine tuple still settles, `applied` is
`created`, and `turn_ordinal` is **1** — proving none of the four refusals
consumed a position in the seat's sequence.

### 2. Existing checks retained (boundary item 2)

`settling_a_bounded_turn_leaves_the_seat_live_and_the_run_open` and
`settling_a_bounded_turn_reads_only_the_claimed_current_window` are unchanged in
substance. The only edit is mechanical: `observe_post_turn_status` now returns
the `TimelinePosition` it appended, so the new matrix can claim that exact
position as a forged response instead of counting sequences by hand. Its one
prior call site now reads `let _ = observe_post_turn_status(...)`. No setup was
duplicated into a new framework.

### 3. Paseo adapter contract regression (boundary item 3)

`crates/kontor-runtime-paseo/tests/contract.rs` —
`message_only_the_echoed_client_id_positions_the_kontor_message`, following the
delivery value end to end on the recorded fixture and the current normalization
path:

1. the send puts the durable Kontor `MessageId` on the wire as Paseo's
   `messageId` (asserted from the recorded wire payload, not inferred);
2. Paseo echoes it as `clientMessageId` on the resulting user message, and the
   acknowledgement's position is that entry's;
3. canonical history normalizes that exact id onto that exact position.

The test seeds a **decoy** first: a pre-existing user message whose *provider*
`messageId` is the very id the send is about. Reading the wrong field still
yields an acknowledgement with a plausible position — just the wrong one — so
the decoy is the assertion rather than the setup. Without it the test would pass
under M1.

## Mutation record

Each mutant was applied to otherwise clean production source, run, then restored
and re-verified green before the next was applied. Restores were byte-exact
against a pre-mutation copy and confirmed with `git status`.

| Mutant | Deliberate defect | Killed by | Red assertion |
| --- | --- | --- | --- |
| M1 | normalize provider `messageId` instead of `clientMessageId` | `message_only_the_echoed_client_id_positions_the_kontor_message` **and** `wire::tests::only_the_client_message_id_addresses_a_kontor_message` | ack position `left: 1, right: 2`; subject `left: None, right: Message(...)` |
| M2 | drop `EventSubject::Message(message_id)` at the claimed request position | `settling_a_bounded_turn_refuses_a_forged_current_window` | `a different valid message id: left: 200, right: 409` |
| M3 | drop the requirement that the claimed response be the last non-state/non-log turn event | `settling_a_bounded_turn_refuses_a_forged_current_window` (also kills `..._leaves_the_seat_live_and_the_run_open`) | `a response position that is no longer terminal: left: 200, right: 409` |
| M4 | permit `runtime_proof: None` to reach settlement | `settling_a_bounded_turn_leaves_the_seat_live_and_the_run_open` | `missing current-turn evidence refuses before any runtime effect: left: 18, right: 17` |

### Mutant definitions

* **M1** — `crates/kontor-runtime-paseo/src/wire.rs`, `normalize_entry`:
  `entry.item.client_message_id` → `entry.item.message_id`.
* **M2** — `crates/kontor-daemon/src/applications.rs`, `prove_current_turn`:
  delete `&& event.subject == EventSubject::Message(message_id)` from the
  message-match condition.
* **M3** — same function: delete
  `|| last_turn_position != Some(response_position)` from the closing refusal.
* **M4** — `settle_turn`: replace the `runtime_proof` `ok_or_else` refusal with a
  fabricated `TurnRuntimeProofRequest` fallback, so a missing proof reaches
  `prove_current_turn` instead of failing closed before it.

### What M2 and M3 proved beyond the status code

Under both mutants the forged settlement did not merely return 200 — it recorded
a real turn (`applied: created`, `turn_ordinal: 1`) **and fanned out a dispatch
to `builder`**. The zero-write assertions in the matrix are therefore load
bearing, not decoration: the production consequence of either mutant is a
downstream seat started off a forged proof.

## Commands

Baseline and post-change, all from the ASMA-8203 worktree at `f78d041`:

| Command | Result |
| --- | --- |
| `cargo test -p kontor-daemon --test loopback_api settling_a_bounded_turn_refuses_a_forged_current_window -- --exact` | PASS, 1/1 |
| `cargo test -p kontor-daemon --test loopback_api settling_a_bounded_turn` | PASS, 3/3 |
| `cargo test -p kontor-runtime-paseo --test contract message_only_the_echoed_client_id_positions_the_kontor_message` | PASS, 1/1 |
| `cargo test -p kontor-runtime-paseo --lib only_the_client_message_id_addresses_a_kontor_message` | PASS, 1/1 |
| `cargo test -p kontor-runtime-paseo` | PASS, 112 lib + 210 contract, 0 failed |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy -p kontor-daemon --tests` | clean |
| `cargo clippy -p kontor-runtime-paseo --tests` | clean |

## Open-question ledger

### OQ-8203-02 — a tuple spanning two turns settles — OPEN

* **Subject:** `prove_current_turn` accepts a settlement that names an *older*
  message at its true position paired with a *newer* turn's terminal response.
* **Attaches to:** Jira ASMA-8203 and
  `POST /v1/projects/{project_id}/agent-runs/{agent_run_id}/turns:settle`,
  specifically `Services::prove_current_turn` in
  `crates/kontor-daemon/src/applications.rs`.
* **Why the state is ambiguous:** this is not one of M1–M4 and no mutant
  survived; it is a shape found while building the negative matrix, on
  **unmutated** production code. The scope record's implementation boundary
  admits production changes "only if a mutant survives due to a real behavior
  gap", and its non-scope forbids weakening the guard but is silent on
  strengthening it. Whether this belongs to ASMA-8203 or to a separately
  admitted ticket is the scope owner's call, not this seat's.
* **Evidence (reproduction, run at `f78d041` with no mutant applied):** two
  completed turns on one seat — older `(message 1:7, response 1:8)`, newest
  `(message 1:9, response 1:10)`. Settling with
  `{message_id: <older>, message_position: 1:7, response_position: 1:10}`
  returned **`200 OK`**, recorded `applied: created`, `turn_ordinal: 1`, and
  dispatched a follow-up to `builder`.
* **Why it passes today:** every existing condition holds. The scan is
  exhausted; `message_matches == 1` because the older id really is at 1:7;
  `response_matches == 1` because 1:10 really is a terminal provider response;
  and `last_turn_position == response_position` because 1:10 really is the tail.
  Nothing examines the newer Kontor-addressed user message at 1:9 sitting
  *inside* the claimed window.
* **Why it matters:** it is the exact failure `prove_current_turn`'s own doc
  comment says it prevents — "prove that the message named by the caller is the
  current completed turn, not a delayed prior notification" — and the scope
  record's non-scope item "do not infer the current turn from the latest
  arbitrary message or response". A caller holding an older message id can
  borrow the current terminal response and settle the seat's newest work against
  the wrong turn.
* **Options seen:** (a) fix here — refuse when any event strictly between
  `message_position` and `response_position` carries `EventSubject::Message(_)`,
  and add the case to the matrix; (b) admit a separate ticket with its own
  acceptance cases and leave ASMA-8203 as the test-only hardening the scope
  record specified; (c) accept as designed and document the narrower guarantee.
* **Disposition:** none. No production change was made. The probe that produced
  the evidence above was removed from the tree rather than committed, because a
  test asserting the current behaviour would enshrine it and a failing test
  would be a broken suite. The reproduction is recorded here verbatim so it can
  be re-run.

OQ-8203-01 remains RESOLVED as recorded in the scope record; nothing in this
phase disturbed it.

## Non-scope confirmation

No echo added to Paseo's request wire. No second proof scanner beside
`prove_current_turn`. No inference of the current turn from an arbitrary latest
message. No weakening of fresh binding attestation, `waiting_input`, same-epoch
ordering, exact message identity, response terminality, or single-use
settlement. No migration and no new persistence row. No transcript prose was used as
runtime evidence.

Within *this* scope no rebuild or redeploy was triggered, because no production
code changed. That is no longer true of the branch as a whole: the OG-058 work
changes production error semantics, and its own record carries the build, the
atomic deployment and the live readback that consequently became mandatory.

## Handoff

Superseded by `OG-058-CLOSURE.md` for everything after this point in the branch's
life. The settlement note below still stands.

The ASMA-8203 implementation turn is to be settled through the supported Kontor
surface after this seat's response is terminal, by a post-turn control caller —
per OQ-8203-01's disposition, this seat does not guess its own not-yet-existing
response position. **The `kontor` MCP server failed to connect in this session
(`CONNECTION_CLOSED`), so that settlement could not be performed from here and
the task-local live proof is outstanding.**
