Artifact: `high-change`

# OG-058 closure: a delivered message reported as "nothing was changed"

Date: 2026-09-17
Task: Jira `ASMA-8203` / Kontor `01a0ac9d-a96d-75b2-9803-169d4f2e531f`
Branch: `feat/ASMA-8203-attach-runtime-message-proof-to-delivery-turns`
Baseline: `f78d041`
Companion records: `HIGH-SCOPE-RECORD.md`, `HIGH-CHANGE-RECORD.md`

## The gap, reproduced

`kontor_session_message_send` → `POST /v1/sessions/{agent_run_id}/messages` could
put a message into a live seat and then refuse the call with HTTP 503,
`code: "unavailable"`, whose action reads **"retry once the dependency answers;
nothing was changed"**. The message had changed everything: it was in the seat.

Captured verbatim from the pre-fix code path (mutant G4 red run):

```json
{"code":"unavailable",
 "rule":"the session's runtime could not be reached",
 "action":"retry once the dependency answers; nothing was changed"}
```

…on a request where the same test proves the message is on the session timeline
exactly once and a replay of the key returns the original acknowledgement.

Two distinct routes reach it:

1. **Delivery.** The runtime commits the message and the acknowledgement is lost
   (`fake.rs` lost-ack; Paseo's channel dying mid-send). Reported as
   `RuntimeError::Transport`.
2. **Readback.** The send is acknowledged, and the post-send `inspect` — or the
   observation persist that follows it — fails. Reported as whatever the
   readback failed with, losing the fact that delivery already succeeded.

In production the second and worst variant is Paseo-specific: when the channel
dies during a send, the adapter's confirming canonical read is attempted over
*the same dead channel*, and its `Transport` error was propagated with `?`,
laundering a known-uncertain delivery into a channel-only fact.

### Why it mattered

`ApiErrorCode`'s own contract is that "a client branches on this and on nothing
else". The warning against resending lived only in the free-text action, and the
code a caller actually branches on said *nothing happened*. A caller following
its own contract resends under a fresh idempotency key, and one instruction
becomes two turns in a live seat's transcript.

## The correction

No second verifier, no new scan, no new settlement path. The existing
`RuntimeError::DeliveryConfirmationUnknown` already meant exactly this; what was
missing was that adapters did not raise it and the API did not distinguish it.

| File | Change |
| --- | --- |
| `crates/kontor-api/src/error.rs` | New `ApiErrorCode::DeliveryUnconfirmed` (`"delivery_unconfirmed"`, 503) whose action names the unsafe act. `DeliveryConfirmationUnknown` now maps to it instead of `Unavailable`. `Unavailable` keeps "nothing was changed" and is documented as usable only before an effect is attempted. |
| `crates/kontor-runtime/src/fake.rs` | A committed message whose acknowledgement is lost now reports `DeliveryConfirmationUnknown`, not `Transport`. |
| `crates/kontor-runtime-paseo/src/adapter.rs` | New `unconfirmed_after_delivery`, applied to both post-delivery `reconcile_message` sites. Only `Transport` is re-labelled; `DuplicateMessage` is a *definite* answer and passes through untouched. |
| `crates/kontor-api/src/sessions.rs` | New `after_delivery`, applied to the post-send `inspect` and the observation persist. Keeps the code, replaces the claim: the message was delivered, replay the same key, never resend under a new one. |

`kontor_turn_settle` is untouched and remains the sole settlement authority. The
new loopback regression asserts it explicitly: after a delivery whose readback
failed and was then repaired by replay, `list_settled_turns` is empty.

## Regressions

| Test | Statement |
| --- | --- |
| `kontor-daemon` · `a_lost_acknowledgement_is_replayed_without_a_second_native_effect` | (corrected) the committed-then-lost delivery reports `delivery_unconfirmed`, and its action neither claims "nothing was changed" nor omits "do not resend" |
| `kontor-daemon` · `a_failed_readback_after_delivery_never_reports_the_message_as_unsent` | (new) the readback half end to end: truthful rule and action, message on the timeline exactly once, replay returns 200 without a second native effect, and **no role turn recorded** |
| `kontor-runtime-paseo` · `message_a_confirmation_read_that_cannot_answer_is_unconfirmed_delivery` | (new) both production arms — accepted-send-then-dead-read, and dead-send-then-dead-read — are unconfirmed delivery, never a bare channel fault, and never a licence to resend |
| `kontor-tests-contract` · `lost_ack_retry_returns_original_message_once` | (corrected) the *shared* adapter contract now binds every adapter, not just Paseo |
| `kontor-api` · `an_unknown_message_confirmation_never_claims_nothing_changed` | (corrected) asserts the code differs from `Unavailable`, which is what a client branches on |
| `kontor-api` · `only_the_pre_effect_code_may_promise_that_nothing_changed` | (new) only the pre-effect code may make that promise |

## Mutation record

Each applied to otherwise clean production source, run, restored, re-verified.

| Mutant | Deliberate defect | Killed by | Red assertion |
| --- | --- | --- | --- |
| G1 | map `DeliveryConfirmationUnknown` back to `Unavailable` | `an_unknown_message_confirmation_never_claims_nothing_changed`; `a_lost_acknowledgement_...` | `left: "unavailable", right: "delivery_unconfirmed"` |
| G2 | fake reports committed-then-lost as `Transport` again | `lost_ack_retry_returns_original_message_once`; `a_lost_acknowledgement_...` | `left: Transport{...}, right: DeliveryConfirmationUnknown{...}` |
| G3 | drop the `unconfirmed_after_delivery` re-labelling | `message_a_confirmation_read_that_cannot_answer_is_unconfirmed_delivery` | `...not a dead channel: Transport { rule: "acknowledgement was lost after the runtime may have accepted it" }` |
| G4 | post-delivery readback speaks as an ordinary refusal | `a_failed_readback_after_delivery_never_reports_the_message_as_unsent` | the verbatim `"nothing was changed"` envelope quoted at the top of this record |

## Test-double change

`fake.rs` gains `fail_next_inspect()`, a non-queue hook mirroring the existing
`lose_next_send_ack()`. The strict script queue cannot express "the send lands
and the readback that follows it fails": a step naming the inspect is refused by
the send it must pass through first. The strict queue is untouched.

## Deployment verification

`kontor-mcp` was rebuilt (`cargo build --release -p kontor-mcp`) and installed
**atomically**: staged to `~/.local/bin/.kontor-mcp.incoming.$$` on the same
filesystem, `chmod 755`, then `mv -f` — a `rename(2)`, so a running process keeps
its own now-unlinked inode, every new exec gets the new file, and no reader can
observe a half-written Mach-O.

```
before: inode=494068667 size=9721104
staged: inode=494175119 (same fs: yes)
after:  inode=494175119 size=9535968   ← new inode, as an atomic replace requires
codesign -v: valid on disk; satisfies its Designated Requirement
```

Provenance of what is now installed:

```
HEAD                f78d041 (the production changes above are uncommitted on the branch)
deployed sha256     f2d3367f8f115e2ab3fad56c3dc7002ea6086f42938371cbb1515449a1cf47dc
target/release      f2d3367f8f115e2ab3fad56c3dc7002ea6086f42938371cbb1515449a1cf47dc  (identical)
final inode         494175119
```

Confirmed by a direct release rebuild after all formatting and evidence edits
were complete: `cargo build --release -p kontor-mcp` reported
`Finished release profile in 0.26s` — a no-op, proving no production source had
changed since the deployed artefact was built — and the byte comparison against
the installed path still reports `MATCH`. `cargo fmt` had touched only two test
files, neither of which is compiled into this binary.

Guard verified **from `/Users/igor/.local/bin/kontor-mcp` itself**, not from
`target/release` and not from a copy — 9/9 before the deploy and 9/9 after:
permitted reads and scoped consultation tools `allow`; shell execution,
delegation, `kontor_turn_settle`, a foreign server using the same tool name, the
wrong hook event, malformed input and empty input all `deny`, every case exiting
0. Five repeated execs of the final pathname: all exit 0, no signal.

Serve-profile gate, driven over stdio from the same pathname:

| Profile | Tools | `kontor_turn_settle` served |
| --- | --- | --- |
| `consultation` | 4 | no |
| `worker` | 18 | **yes** |
| `leadership` | 4 | no |

The server also reached the realm at `http://127.0.0.1:7717/` on every profile.

### The SIGKILL was not reproduced — read this before concluding

The stated reason for the atomic step is that a byte-identical, in-place-installed
binary was observed being SIGKILLed while a copied inode ran correctly. **Two
attempts to reproduce that on this machine both failed:**

1. `cat src > dest` over an idle binary's existing inode → ran, exit 0,
   signature valid, bytes identical to source.
2. `cat src > dest` over a binary while a live process had it mapped → the
   running process **survived**; overwrite exit 0.

So the atomic deployment is delivered as instructed and is correct practice
regardless — it is the only way to avoid a torn binary and a stale signature
cache — but it is **not established that it addresses the observed SIGKILL**,
because that SIGKILL did not occur here under either mechanism. Whatever killed
that process has another cause, and the atomic step should not be treated as
having fixed it. This is recorded as OQ-8203-03.

## Open-question ledger

### OQ-8203-03 — the SIGKILL mechanism is unidentified — OPEN

- **Subject:** why a byte-identical in-place-installed `kontor-mcp` was SIGKILLed
  while a copied inode ran.
- **Attaches to:** `~/.local/bin/kontor-mcp` deployment, and the `kontor` MCP
  server entry in `.mcp.json`.
- **Why the state is ambiguous:** neither in-place mechanism reproduced it here
  (evidence above). The binary is `adhoc, linker-signed`, and the installed file
  carries a `com.apple.provenance` xattr, so a Gatekeeper/AMFI provenance or
  signature-cache interaction remains plausible but unproven. A fix cannot be
  claimed for a mechanism that was never observed.
- **Options seen:** (a) capture the actual failure — `log stream --predicate
  'senderImagePath CONTAINS "AMFI"'` or a crash report from
  `~/Library/Logs/DiagnosticReports` at the next occurrence; (b) leave the atomic
  install in place and treat any recurrence as new evidence; (c) attribute it to
  the in-place write and close — **not supported by anything measured here**.
- **Disposition:** none taken. The atomic deployment stands on its own merits.

### OQ-8203-04 — the Jira publication path makes the same false claim — OPEN

- **Subject:** whether the Jira description/publication path should also report
  a lost post-write confirmation as `delivery_unconfirmed` rather than
  `unavailable`.
- **Attaches to:** `/v1/projects/{project_id}/epics/{epic_id}/jira/description:apply`
  and the native Jira connector's error mapping.
- **Why the state is ambiguous:** it is the identical defect class, and the code
  that fixes it now exists — but the instruction scoped this work to
  `kontor_session_message_send`, and the Jira tests that reveal it are failing
  at baseline for an unrelated environmental reason, so the path could not be
  exercised green here either before or after a change.
- **Options seen:** (a) extend the same mapping to the Jira connector under a
  separate ticket, once its suite is green again; (b) leave it, accepting that
  a lost Jira confirmation still advises "nothing was changed"; (c) fold it in
  here — rejected, it widens an already-expanded scope into an area whose tests
  do not currently pass.
- **Disposition:** none taken; no Jira code was changed.

### OQ-8203-02 — a tuple spanning two turns settles — OPEN

Unchanged and still open; see `HIGH-CHANGE-RECORD.md`. Nothing in this phase
touched `prove_current_turn`.

## Test results

| Suite | Result |
| --- | --- |
| `kontor-daemon` (full, incl. 331-test `loopback_api`) | 323 passed, **7 failed — all 7 pre-existing**, see below |
| `kontor-api` | pass |
| `kontor-runtime` | pass |
| `kontor-runtime-paseo` (112 lib + 211 contract) | pass |
| `kontor-mcp` | pass |
| `kontor-tests-contract` | pass |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy` on every crate touched here | clean |

## Known pre-existing breakage, untouched

**Verified, not assumed.** A throwaway worktree was created at clean `f78d041`
(`git worktree add --detach`, separate `CARGO_TARGET_DIR`, no changes applied)
and the seven failing tests were run there. **All seven fail identically at the
baseline**, so none is caused by this work:

```
test result: FAILED. 0 passed; 7 failed; 0 ignored; 322 filtered out
  a_body_overtaken_during_a_lost_confirmation_is_refused
  a_human_authored_body_is_preserved_until_replacement_is_authorized
  a_publication_that_never_landed_is_still_refused
  a_publication_whose_confirmation_was_lost_is_settled_by_refetch
  an_epic_placeholder_body_is_typed_reported_and_repairable
  reconcile_plan_refuses_to_call_a_placeholder_body_converged
  replaying_a_partial_admission_delivers_its_durable_follow_up
```

Six are Jira description/publication tests that refuse with
`"the configured native Jira connector could not answer"` (503 where 200 was
expected); the seventh asserts a hardcoded `expected_task_revision: 1` against a
task that has since moved. None references the session-send path, the error
codes, or the hooks changed here.

`cargo clippy --workspace --tests` additionally fails to compile
`kontor-tests-e2e` (test `pilot`): `missing field observed_identity in
initializer of kontor_jira::jira::JiraResponse`. Neither `crates/kontor-jira` nor
`tests/e2e` is touched by this work.

### Worth a follow-up: the same lie, in the Jira path

The baseline output above is itself evidence of OG-058's shape *outside* the
scope closed here. `a_body_overtaken_during_a_lost_confirmation_is_refused`
refuses with:

```json
{"code":"unavailable",
 "rule":"the configured native Jira connector could not answer",
 "action":"retry once the dependency answers; nothing was changed"}
```

That is the same `Unavailable`-after-a-write pattern, on the Jira publication
path, in tests whose own names say the confirmation was lost *after* the write
landed. It was not touched here — the instruction scoped this work to
`kontor_session_message_send` — but `ApiErrorCode::DeliveryUnconfirmed` now
exists and is the right code for it. Recorded as OQ-8203-04.

## Live observation → settlement attempt (real, not simulated)

Candidate commit `e4a663ab4ed29e249216b78e6614a0477ebb7231`, pushed to
`origin/feat/ASMA-8203-attach-runtime-message-proof-to-delivery-turns`.

### Preserved identity

Read from the live realm `01a00649-9ee6-73e0-ba1b-6a6c35cfd065`:

```
task_id      01a0ac9d-a96d-75b2-9803-169d4f2e531f
title        Attach runtime message proof to delivery turns
state        in_progress        phase  high-implementation      revision 2
jira_binding {"state":"confirmed","jira_key":"ASMA-8203",
              "readback_hash":"94f9809d5475f5a86174f3d174aecc743762d5b010e39f37f94366a2a7e62208",
              "confirmed_at":"2026-09-16T23:59:51.926606Z","revision":2}
```

The UUID↔key binding is `confirmed` and its `confirmed_at` predates every change
in this branch, so the identity is preserved, not re-derived. Nothing in this
work touches the identity path.

### The implement turn is not settled, and cannot be from this seat

Settled turns on this task — one row, the scope turn:

```
ordinal role_slot message_id            msg     resp    evidence_hash    settled_at
1       scope     455ba95b-ae35-73a1…   5:720   5:759   795c05f520a766…  2026-09-17T01:55:53Z
```

Seats on the team run:

```
01a0acf9-6636  scope      waiting_input  confirmed
01a0acf9-8531  implement  running        confirmed   ← this seat, mid-response
01a0acf9-998d  verify     parked         terminal/abandoned
```

A settlement was attempted through the supported surface
(`POST /v1/projects/{project}/agent-runs/01a0acf9-8531-…/turns:settle`) and the
live daemon refused:

```json
{"code":"revision_conflict",
 "rule":"the exact settling seat is not freshly waiting after the named current turn"}
```

This is correct and is the resolved disposition of OQ-8203-01 in the scope
record: the implement seat is `running` because it is still producing this very
response, so no terminal response event exists at any canonical position yet,
and `prove_current_turn` requires `waiting_input` plus an exact terminal
response. **No implement receipt can exist until after this response is
terminal**, and it must be produced by a post-turn control caller. Fabricating
positions to obtain one is precisely what mutants M2 and M3 prove must be
refused.

Incidentally a live proof of the guard and of the zero-write boundary this
ticket asserts: after the refusal the `role_turns` table still holds exactly the
one scope row, and no dispatch was recorded.

## Gate status — OG-058 and OG-059 remain OPEN

Neither gap is closed by this record. The work above is the implementation
evidence only; verification and audit gates remain required and unmet. OG-059 is
untouched by this branch. Nothing here should be read as a closure claim.
