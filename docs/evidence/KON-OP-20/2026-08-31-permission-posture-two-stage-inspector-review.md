# KON-OP-20 — two-stage OpenCode delivery (inspector review, 2026-08-31)

- **Task:** `01a02a7f-8e47-7682-be52-1b9f2a632ac4`, `ai_short_name: permission posture`.
  **Not ASMA-7968.**
- **Role:** inspector (AUD), agent run `01a0306f-0816-7ab3-a790-036a6ef11cdc`,
  team run `01a0306e-6de5-7bb2-a8b6-18a8dd01bbb9`, delivery seat binding
  `01a0306e-8fda-75d2-9f47-5ce1add8016b` on node `01a0306e-6de7-7c90-aaa6-4995ea6dc074`.
- **Reviewed:** `feat/KON-OP-20-permission-posture-at-spawn` at `e2863bb`
  (one commit beyond the `32fccde` named in the handoff), baseline `e814661`.
  Outer repo `7832934f`.
- **Verdict:** **BLOCKED.** Nine blocking findings. The first is the load-bearing
  external fact the builder asked to be pressed on, and it does not hold.

## B1 — BLOCKING, decisive · the rendered permission never reaches OpenCode

The soundness of the whole two-stage design rests on one claim: *the daemon
replays the persisted `providerOptions.permission` into `session.promptAsync`,
and OpenCode installs it before evaluating a tool call.* Verified against the
installed bundles, and it is **false at two independent points**.

**1. The v2 SDK drops it before the wire.** Paseo imports
`@opencode-ai/sdk/v2/client` (`opencode-agent.js:1`). That client's `promptAsync`
declares an explicit body allow-list:

```
messageID, model, agent, noReply, tools, format, system, variant, parts
```

`permission` is not in it, and the call site declares no `allowExtra`.
`buildClientParams` places only mapped keys, honours only the `$body_` /
`$headers_` / `$path_` / `$query_` prefixes, and **silently discards everything
else**. So the `...(permission ? { permission } : {})` spread at
`opencode-agent.js:2487` and `:2768` is computed correctly, passed correctly, and
then dropped.

**2. The server would not install it anyway.** In OpenCode 1.18.15,
`SessionPrompt.prompt` builds the session's rules from `t.tools` only:

```js
let B=[]; for(let[Q,C]of Object.entries(t.tools??{})) B.push({permission:Q,action:C?"allow":"deny",pattern:"*"});
if(B.length>0) O.permission=B, yield*o.setPermission({sessionID:O.id,permission:B});
```

There is no `t.permission` handling on the `prompt_async` route.

**No other route carries it.** `client.session.create({ directory })`
(`opencode-agent.js:989`) passes no permission. `client.session.update` is called
only to set the archive timestamp (`:1159`). `server-manager.js` contains zero
occurrences of `permission`. `buildOpenCodePermissionRules` is referenced only in
`options.js` (its definition) and the two dropped prompt sites.

**Consequence.** A seat launched through this path runs under whatever OpenCode's
own configuration layers resolve — the exact ambient-merge problem the design was
built to eliminate. On this operator's host that is the 2026-08-22 stopgap; on a
clean host every unmatched tool takes the evaluator's `{action:"ask"}` default,
which reproduces the original wedging outage this task exists to fix.

**Why the gates cannot see it.** `opencode_delivery_gates` (adapter.rs:3517)
requires `PaseoServerInfo::supports_provider_options`, which is a **version
comparison only** (`version_at_least(version, PASEO_PROVIDER_OPTIONS_VERSION)`,
wire.rs:357). `prove_first_turn` proves the *turn was accepted*, not that a policy
was applied. The builder's own 2026-08-31 evidence had this exactly right before
the current disposition reversed it: *"a successful create with no
`provider_options_invalid` proves the options were accepted and schema-valid. It
does not prove the spawned process applied them."* Nothing since then changed
that; the replay reading did not survive checking.

**Constructive.** The mechanism exists, on other routes. The v2 SDK **does**
allow-list `permission` on `session.create` (with `parentID, title, agent, model,
permission, workspaceID`) and on `session.update` (`title, permission, time`),
and OpenCode's `SessionHttpApi.update` installs it via `setPermission`, merging
with the session's existing rules. Closing this needs Paseo to attach the
permission at session create/update rather than per prompt — an upstream change —
or Kontor must stop treating turn-acceptance as proof of policy application and
re-fail-closed until an applied-acknowledgement exists.

## B2 — BLOCKING · census-zero returns `Transport`, so a retry may create a second seat

`recover_launch` (adapter.rs:3689) returns, for zero matches:

```rust
0 => Err(RuntimeError::Transport { rule: "acknowledgement was lost and no agent carries this launch's labels yet" }),
```

and `launch` releases the admission on **every** error (adapter.rs:5462-5466):

```rust
let outcome = self.launch_admitted(request, &declared, generation, &posture).await;
if outcome.is_err() { self.lock().admissions.release(request); }
```

So the seat is immediately re-admissible. A retry re-enters `launch_admitted`,
censuses zero again while the first create is merely not yet visible, and sends a
**second create** — "how one seat acquires two sessions", which the
`create_opencode_seat` comment claims this design prevents. The guarantee holds
only *within* one invocation.

`recover_launch`'s own doc comment already states the correct semantics — *"The
receipt stays confirmation-unknown and reconciliation looks again"* — and the code
does something else. `RuntimeError::DeliveryConfirmationUnknown` exists
(`kontor-runtime/src/adapter.rs:226`) and the AO adapter already uses
`ConfirmationUnknown` for this same hazard, so this is a departure from an
established in-repo pattern, not a missing capability.

**Crash dimension.** `kontor-daemon/src/runtimes.rs:613` builds every production
adapter from `PaseoCheckpoint::fresh(INITIAL_GENERATION, host_key)`; startup does
not restore serialized adapter state. An in-memory quarantine would therefore not
survive a restart, and the only record of an ambiguous create is the agent label —
which, in the ambiguous window, is by definition not yet visible. There is no
durable local ambiguity marker at all.

**Required.** Zero-after-ambiguous-create must be `DeliveryConfirmationUnknown`,
backed by a pending-launch claim written to Realm storage and re-derived and
reconciled at startup, blocking blind re-create until reconciliation establishes
one exact match or proves no effect. Acceptance must cover **same-process retry
and restart between the ambiguous create and visibility**.

## B3 — BLOCKING · a lost archive acknowledgement skips the terminal readback

`compensate_unproved_seat` (adapter.rs:3665):

```rust
self.transport.request(&archive).await?;   // <- escapes here
let agent = self.fetch_agent(native_id).await?;
if agent.is_archived() { return Ok(()); }
Err(RuntimeError::DeliveryConfirmationUnknown { .. })
```

The `?` on the archive request returns raw `Transport` **before** the readback the
function's own doc promises. The three lines beneath it are the correct pattern.
`an_unconfirmed_archive_refuses_recoverably` scripts a successful archive response
plus a live readback; it never drops the archive acknowledgement, so this path is
untested. Required: a lost-archive-ack test — read back archived ⇒ cleanup
confirmed; read back live or ambiguous ⇒ `DeliveryConfirmationUnknown` with the
pending launch quarantined — never generic `Transport`, never a blind recovered
first-turn resend.

## B4 — BLOCKING · the first-turn proof reads one page and calls absence absence

`first_turn_on_timeline` (adapter.rs:3637) fetches exactly one `Tail` page of
`MAX_HISTORY_PAGE = 500` (wire.rs:124) and returns `entries.iter().any(...)`.
Absence then authorizes compensation and archive. `scan_canonical` in the same
file documents and handles the counterexample — a busy turn can emit more than one
page before an acknowledgement settles — by walking backward, enforcing a single
epoch, and returning `DeliveryConfirmationUnknown` when its page budget ends
before a complete scan. The first-turn proof needs equivalent bounded canonical
pagination before absence can authorize archiving a seat that may have started.
Required: a multi-page test with the initiating message older than the tail, plus
mutants for truncation and epoch change. Do not bind early merely to reuse the
helper.

## B5 — BLOCKING under the epic's fix-all-mismatches scope · dead verifier and present-tense docs describing a deleted mechanism

The verifier cluster is a closed island, reachable from nothing:

| Symbol | Repo-wide hits |
| --- | --- |
| `verify_composed_posture` | definition + 3 doc references, **no call site** |
| `verify_composed_posture_in` | definition + called only by the above |
| `ConfigLayers`, `effective_permission`, `permission_of` | only inside that island |
| `compose_permission_block` | **1 hit — a doc comment citing a function that does not exist** |

The test module starts at `seat_mcp.rs:673`; every cluster reference is below line
600, so **no test calls the entry point** either (only `strip_jsonc` is touched).
`clippy` cannot see any of it because the items are `pub` in a `pub mod` — which is
precisely how dead public surface accumulates unnoticed.

Present-tense API documentation that contradicts current source:

| Location | Says | Actually |
| --- | --- | --- |
| `seat_mcp.rs:148-154` (`///` on `compose_for_seat`) | "opencode gets the permission block its declared posture rendered to"; "the block a seat reads" | body writes nothing and does `let _ = posture;` |
| `seat_mcp.rs:178` | cites `compose_permission_block` | no such function |
| `posture.rs:206-212` | admission requires per-agent **environment**, the **resolved binary**, and **resolved permission equal to this block**; cites `PaseoAdapter::prove_opencode_posture` | the gate is a version check; `prove_opencode_posture` does not exist (1 hit — this comment) |
| `posture.rs:219-221` | "Nothing on the delivery path calls it directly" | `adapter.rs:3539` and `client.rs:1078` both call `render_posture` directly |
| `posture.rs:231` | "Only opencode reads a written permission block" | nothing is written |
| `wire.rs:191-199` (`SEAT_POSTURE`) | digests "posture, owned config and environment"; the census adopts on this digest | **1 hit repo-wide — its own definition**; the census matches `LAUNCH_INTENT` (6 callers) |
| `contract.rs:1752` | "Its posture lives in a Kontor-owned root outside it" | the owned root is deleted |

Require removal of the dead verifier, types and helpers, and correction of these
comments, the contract-test comment and the evidence table. The shipped
`providerOptions` design makes a file-based ambient verifier obsolete, and — given
B1 — actively misleading about where posture comes from.

## B6 — BLOCKING · the retired design left broken public API docs and a dead public field, and rustdoc proves it

This one is machine-checkable. Running

```
RUSTDOCFLAGS="-D rustdoc::broken_intra_doc_links" cargo doc -p kontor-runtime-paseo --no-deps
```

**fails, exit 101**, with four unresolved links — each naming a symbol the
owned-root/environment design took with it:

| Broken link | Where | Target |
| --- | --- | --- |
| `PASEO_SEAT_ENVIRONMENT_VERSION` | `wire.rs:87` | 1 hit repo-wide — this link |
| `Self::supports_seat_environment` | `wire.rs:353` | 1 hit repo-wide — this link |
| `crate::posture::SeatConfigRoot::for_seat` | `adapter.rs:318` | 1 hit repo-wide — this link |
| `PaseoAdapter::capabilities` | (4th, not previously reported) | unresolved |

plus six warnings, five of them public documentation linking to private items
(`paseo_mode`, `consultation_permission_mode`, `consultation_route_permission_mode`).

**A dead public field with it.** `PaseoConfig::state_root` (`adapter.rs:319`)
occurs inside `kontor-runtime-paseo` only as its own declaration and five *test*
constructors (`6961, 7067, 7153, 7231, 7362`). There is no production read. The
daemon's heavy use of `config.state_root` is its own `DaemonConfig`, a different
struct; `runtimes.rs:476` puts the path into `SeatMcp`, which does use it.
`label::SEAT_POSTURE` is dead in the same way (B5).

**Why this is blocking rather than tidy-up.** The builder evidence states the
owned-root and environment code is *deleted*. It is not, while these public
surfaces still name it — and unlike the prose mismatches in B5, this one is proved
by the toolchain rather than by reading. `clippy` cannot catch it: broken
intra-doc links are a rustdoc lint, and the acceptance ran clippy and fmt only.

**Required.** Remove the residue or give a truthful supported-compatibility
justification for each retained surface, and add the rustdoc gate above to CI so
the next retirement cannot leave the same trail. It is a cheap check that would
have caught all four today.

## B7 — BLOCKING · the launch intent does not cover the prompt it launches

`LaunchIntent` (posture.rs:354) digests `binding_id`, `agent_run_id`,
`workspace_id`, `role_slot_id` and `config`, and its doc argues at length that
hashing the whole create config beats "a hand-listed subset". That argument is
right and the implementation honours it — but the design deliberately sends **no
`initialPrompt`**, so the prompt is not in the create config, and is therefore
outside the digest by construction. `prove_first_turn` then derives

```rust
let message_id = format!("kontor-first-turn-{}-{}", request.agent_run_id(), request.binding_id());
```

which likewise carries no prompt content. `LaunchAuthority` covers only
ticket/slot/run/binding.

**Failure scenario.** A replay with the same ids and authority but a changed
prompt produces an identical intent digest, so the census adopts the existing
agent; `prove_first_turn` then sends the *changed* body under a message id that
may already exist. `first_turn_on_timeline` matches on `client_message_id` alone,
so a stale turn satisfies the proof for a turn whose content is different — or the
send collides with an earlier ambiguous one.

**Required.** Bind the prompt content hash into the two-stage launch intent and
the message receipt, and refuse a changed intent; or prove at the public
constructor/store boundary that one run+binding can never present different
prompt bytes. Either way with a mutation test — the current suite would not
notice.

## B8 — BLOCKING · every post-create exit before binding strands a live native

Compensation is wired to exactly one failure. In `launch_admitted`:

```rust
let native_id = ... self.create_opencode_seat(delivery).await?;   // the seat now exists
let agent = self.fetch_agent(&native_id).await?;                  // (1) `?` — no compensation
self.verify_agent_placement(&agent, &workspace_id, &labels)?;     // (2) `?` — no compensation
Self::verify_agent_route(&agent, request.model_rung(), request.autonomy())?; // (3) `?` — none
if delivery.is_some() && let Err(unproved) = self.prove_first_turn(...).await {
    self.compensate_unproved_seat(&native_id).await?;             // only here
    return Err(unproved);
}
```

A readback transport loss or not-found at (1), or a placement or route/mode
mismatch at (2)/(3), leaves a **known created native** unbound and unarchived.
`launch` then releases the admission (B2), so a retry can duplicate it or adopt
the same bad live native again and again. This is the same defect class as B2 and
B3, and it is the widest instance: the seat's existence is known here, which is
precisely when compensation is cheapest and most obligatory.

**Required.** A durable, quarantined create-to-bind state, with governed archive
plus terminal readback (or exact recovery) on **every** post-create exit, and
failpoints for agent-readback loss and for each verification failure — not only
first-turn refusal.

Note also that the comment introducing `prove_first_turn` states the B1 claim as
the admission rationale in current source: *"the daemon has taken a turn, which it
runs by replaying the agent's persisted `providerOptions.permission` into
`session.promptAsync` — so the turn that was accepted is a turn that ran under the
policy Kontor sent."* That sentence is the whole soundness argument, and B1
falsifies it. The following comment — "no drift window, because there is nothing
on disk for anything to drift from" — is true about disk and beside the point:
given B1 there is no policy anywhere.

## B9 — BLOCKING · an accepted turn can still end unbound (the inverse of B8)

Recorded after the turn receipt below was settled; it belongs to this review.

`prove_first_turn` returning `Ok` means the seat **has started work**. Eight
fallible steps then stand between that and the binding (adapter.rs:3444-3494):

```rust
let snapshot   = self.bind(...)?;                                   // 1
let observation= self.observation(...)?;                            // 2
let record = PaseoSeatRecord {
    mini_project_id: Self::external_epic_id(&effective_scope)?,     // 3
    plan_item_key:   self.scoped_plan_item_key(&effective_scope)?,  // 4
    workspace_id:    ExternalId::parse(&workspace_id)?,             // 5
    agent_id:        ExternalId::parse(&agent.id)?,                 // 6
    provider_session_id: agent.provider_session_id().map(ExternalId::parse).transpose()?, // 7
    ...
};
{ let state = &mut *self.lock();
  state.admissions.occupy(request, ExternalId::parse(&agent.id)?)?;  // 8, inside the section
```

A malformed native id or provider session id, or an evidence/timestamp
conversion in `observation`, therefore returns `Err` **after work has begun**.
The compensation block fires only on first-turn *failure*, so this path gets no
archive and no readback, and `launch` then releases the admission (B2). The seat
is running, unbound, unowned and re-admissible.

Note `ExternalId::parse(&agent.id)` is evaluated twice — at (6) and again at (8)
inside the critical section — so the same malformed value can fail after the
snapshot and observation have already been built.

**Required.** Every fallible snapshot, observation and record preparation must
happen **before** the seat is prompted. After a typed acceptance the
commit-to-binding path should be infallible, or durably recoverable. Add a
malformed-readback failpoint proving no accepted turn can be left unbound.

## The fixture-unreachable shape generalizes

The builder asked whether the census-fixture defect recurs. It does, and in the
test guarding the double-create invariant:
`a_lost_create_acknowledgement_is_never_answered_by_a_second_create` invokes
`launch` **once** and asserts `count("rpc create_agent_request") == 1`. With a
single invocation that assertion cannot fail, so it does not test what its comment
claims ("resending is how one seat gets two sessions"). It also accepts
`RuntimeError::Transport`, the very variant that makes B2 reachable. The same
shape appears in B3 (archive ack never dropped) and B4 (never more than one page).
The common signature: **the fixture cannot produce the state the assertion rules
out.**

## What is confirmed good

- **F1 closed.** My own deterministic sweep: deleting each of the five floor
  patterns is now caught, by `the_floor_is_exactly_the_five_published_patterns`,
  `every_published_pattern_is_denied_by_literal_name`,
  `an_allowance_must_name_an_exact_floor_pattern` and
  `an_allowance_can_only_flip_a_floor_key_never_add_one`. Tree restored.
- **F5 closed**, exactly as recommended: `PermissionAllowance::parse` now requires
  `DESTRUCTIVE_BASH_DENIES.contains(&pattern)`, so an allowance is a subset of the
  floor by construction and can only flip an existing key.
- **Acceptance reproduced:** paseo lib 107, paseo contract 161, daemon lib 57, all
  passing. I did **not** run the full-workspace clippy/fmt this turn, so those
  figures are the builder's, unverified by me.
- The identity separation, the schema-v5 work, the resolution order and the
  destructive floor itself remain correct.

## Not claimed

No live authenticating OpenCode seat was launched, and none may be. B1 is
established from the installed bundles by source reading, not from a running seat.
OpenCode operator configuration was not modified; the user's global config was
read only.

## Turn receipt

Settled through the governed surface, contrary to the handoff's expectation that
`stale_binding` would block it: `kontor_turn_settle` returned **200**,
`applied: created`, turn `01a054f9-7e66-7e71-ae96-b10f26cda005`, ordinal 1,
`seat_live: false`, with an undispatched follow-up to the `tester` slot
(`01a0306e-e068-78e0-95c7-75528d0cda9a`). Artifacts were rejected once for key
charset — settlement artifact keys accept only lowercase ASCII letters, digits,
`.`, `_` and `-`, so a path or a `repo@sha` is refused. B9 was appended after
that receipt.
