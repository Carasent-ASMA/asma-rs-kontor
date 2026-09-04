# OP-20 — OpenCode delivery re-enabled on an applied-policy acknowledgement

- **Task:** `01a02a7f-8e47-7682-be52-1b9f2a632ac4`, "Deterministic agent
  permission posture at spawn time". Not ASMA-7968.
- **Supersedes:** every fail-closed disposition in this directory, and the
  `2026-08-31-upstream-dependency-applied-permission.md` note, whose dependency
  is now satisfied.
- **Upstream carrier (deploy pin):** ASMA-7869 on exact v0.6.1 —
  **`a07ed03e0`**, parent `a8781451415c065910cc768999a1129222e7204a`, itself parented on the
  `20d7efc…` release tag. The same fix is `661536df9` on main.
  `a07ed03e0` is the commit a deployment carries; `a8781451415c065910cc768999a1129222e7204a` is
  cited below only where a *defect in that earlier revision* is the reason for a
  Kontor behaviour.

  The carrier applies the exact ordered OpenCode policy
  at `session.create` / `session.update`, requires the returned session echo,
  advertises `server_info.features.providerOptionsApplied`, and projects
  `providerOptionsApplied: true` on the exact agent snapshot.

## What ships

One `create_agent_request` is the whole launch. Inside `config`:
`providerOptions.permission` and the typed MCP surface in `mcpServers`. As
**top-level siblings of `config`**: `initialPrompt`, a `clientMessageId` derived
from the launch rather than generated, and `labels` carrying a launch-intent
digest over the whole outgoing message **and** the prompt and its message id.

The envelope is read from `paseo-op20-v0.6.1-backport` at commit `a8781451415c065910cc768999a1129222e7204a`:

- `packages/protocol/src/messages.ts`, `CreateAgentRequestMessageSchema` —
  `initialPrompt`, `clientMessageId` and `labels` are declared as siblings of
  `config`, not fields within it.
- `packages/server/src/server/session.ts`, `handleCreateAgentRequest` —
  destructures `initialPrompt` and `clientMessageId` from the message and passes
  both to `createAgentCommand`.
- the same handler answers with a `type: "status"` frame whose payload carries
  `status: "agent_created"`, the `requestId`, and the agent payload built from
  `liveSnapshot`; `createAgentCommand` awaits `sendInitialPrompt` before that.

Two further facts from the same source shape the gates:
`websocket-server.ts` advertises `providerOptionsApplied: true` in the
`server_info` feature object, and `agent-projections.ts` emits
`providerOptionsApplied` on the agent **only when it is true** — the field is
`true`-or-absent, never `false`. Kontor's reader requires an explicit `true`, so
absence refuses; the `false` case is covered defensively and is a value this
daemon does not send.

The two-stage shape is not restored. With the daemon reporting application on the
agent itself, a separate first turn proves nothing the snapshot does not already
say, and two effects to reconcile instead of one is pure hazard: a create that
lands and a prompt that does not leaves a seat that exists, is bound to nothing,
and has no instructions.

### The two gates

| | Asked | Answers |
| --- | --- | --- |
| Before any native call | does this daemon advertise `providerOptionsApplied`? | the daemon **can** |
| On the returned snapshot | is `providerOptionsApplied` exactly `true` for this agent? | the daemon **did** |

A launch binds on the second. `Some(false)` is the daemon saying it did not;
`None` is a daemon that does not answer. Both refuse.

**Never the version.** Kontor shipped a version-gated path once whose permission
the daemon validated, persisted and dropped before it reached the provider — the
v2 SDK's `promptAsync` allow-lists its body keys, and OpenCode's own
`SessionPrompt.prompt` reads only `t.tools`. The version was correct and the
policy never applied. A release number describes a build; it cannot assert what
that build does with a field.

Not restored either: the environment approach, the owned configuration root, and
the preflight that read files. Those layers merge after anything Kontor writes
and depend on who the seat authenticated as.

### Ambiguity

A create whose answer is lost may have landed, so it is never sent again.
Reconciliation is an exact-label paginated census on the launch intent: one
unbound match **whose first turn is proved on its canonical timeline** adopts;
one match without that proof refuses; one already-bound match refuses; none on a
*complete* enumeration is `DeliveryConfirmationUnknown`; and several — or an
enumeration that did not finish — is quarantined.

**Nothing releases the seat claim on an ambiguous outcome**, including
`agent_create_failed`. Releasing is what would licence a second create for a run
that may already own a native. See the finding below for why the daemon's word
about a failed create is not a statement about the world.

A created seat that fails any check is archived and read back terminal. The
archive acknowledgement is not the cleanup — it can be lost after the daemon
acted — so the readback runs regardless, and only a fresh reading of that exact
agent as terminal counts. A durable bind failure returns
confirmation-unknown, keeping the claim and the intent label so reconciliation
adopts that very agent.

Claude, Codex and Cursor keep the CLI create and readback unchanged.

## Acceptance

- fmt clean; `clippy --workspace --all-targets -D warnings` 0; rustdoc
  broken-intra-doc-link gate for `kontor-runtime-paseo` exit 0.
- `kontor-runtime-paseo` lib 92, contract 166; `kontor-daemon` lib 57.
- **Mutation: 32/32 killed** over the caller gates — twenty on the launch
  wiring, two on the create envelope once it was corrected (a wrong declared
  response type, and `initialPrompt` moved off the message top level), eight on
  the two create-to-bind findings below, and two on the typed
  `agent_create_unresolved` status. Three survived their first
  run, and each survival was the finding:
  - an incomplete census treated as complete stayed green because the test
    asserted only the error *variant*, and "could not enumerate" and "found none"
    share it. The assertion now names the rule.
  - adopting an already-owned match, and adopting one of several, both stayed
    green because neither quarantine branch had a test at all.
  - a digest whose fields run together without delimiters still changed when the
    prompt changed, so the existing assertions could not see the concatenation
    collision. One now can.

## A correction found by reading the evidenced builder, not by a test

The first version of this create declared response type `create_agent_response`
and put `initialPrompt` and `clientMessageId` inside `config`. Every test passed.
They could not have failed: the recorded transport builds its reply from
whatever `response_type` the request declared, and no fixture asserts where the
daemon reads the prompt from.

Against the live daemon it would have failed correlation on every launch, and
the prompt and labels would have been ignored — the same shape as B1, a field
accepted by the harness and never acted on by the thing that matters.

`PaseoRpc::hosted_seat_agent_create` is the evidenced envelope: response type
`status`, with `initialPrompt` and `labels` as siblings of `config`. The
delivery create now matches it, and
`the_delivery_create_matches_the_evidenced_create_envelope` pins the two
together so they cannot drift. Two mutants confirm it — a wrong response type
and a prompt moved off the top level are both caught.

**Now closed.** The placement of `clientMessageId` was recorded here as
unverified, because the ASMA-7869 source was not on this machine at the time. It
is, at `paseo-op20-v0.6.1-backport` at commit `a8781451415c065910cc768999a1129222e7204a`, and it confirms the shipped
envelope: `clientMessageId` is a top-level sibling of `initialPrompt`, exactly as
sent. Nothing about the create is inferred any more.

## Two create-to-bind findings, and what they changed

Both were confirmed against the backport source before any code moved.

**An archive acknowledgement is not the cleanup.** `compensate_invalid_seat`
awaited the archive with `?` before reading the seat back, so an acknowledgement
lost *after* the daemon archived the seat returned a plain transport error,
proved no terminal state, and released the launch claim — licensing a second
create for a run that already had a native. The archive is now attempted, its
outcome logged, and the readback runs either way. Only a fresh reading of that
exact agent as terminal counts; live, unfetchable, or an answer about a different
agent all keep the claim.

**A reported create failure is not evidence that nothing was created**, and
Kontor treats it that way on every daemon. Three revisions matter here, and they
differ:

| Revision | On a post-create prompt failure |
| --- | --- |
| stock 0.6.1 `a8781451415c065910cc768999a1129222e7204a` | `createdAgentId` was captured *after* the prompt, so a throwing prompt left it null. The catch emitted `agent_create_failed` **while the agent ran**, and `cleanupCreatedWorktreeAfterFailedAgentCreate` skipped it too, having been handed a null id. |
| deployed candidate `a07ed03e0` | `onCreated` fires *before* `sendInitialPrompt`, so the id is recorded first. `compensateCreatedAgentAfterFailedCreate` then attempts an exact-agent archive: on success it emits `agent_create_failed`, now genuinely meaning compensation was **confirmed**; on failure it emits the typed `agent_create_unresolved` carrying `requestId` and `agentId`. |
| Kontor | **Does not distinguish them.** Failed, unresolved and unrecognised all take the same census and first-turn proof, and none releases the seat claim. |

The reason Kontor stays conservative is not doubt about the candidate — the
candidate's behaviour is read from the local checkout and is correct. It is that
branching on the word would make correctness depend on which build answered, and
a daemon can be rolled back or replaced under a running plane. One path serves
both, and it costs one directory read on a path that is already the unhappy one.

Kontor also does not adopt the agent the candidate *names* in
`agent_create_unresolved`. That id is recorded for the operator and never bound
on; the agent is found through the census and its own timeline or not at all.
There is deliberately no predicate over the status — a predicate exists to be
branched on.

The same fact reshapes recovery. Because the prompt is sent after the agent
exists, an agent can carry this launch's exact intent and never have been told
anything; adopting it on labels alone would seat a run on a session that sits
idle forever while the launch reports success. An adopted agent must now have the
launch's `clientMessageId` on its **canonical** timeline — scanned backward from
the tail, bounded pages, one fixed epoch. Absent id, unfinished scan, renumbering
mid-scan, or a daemon-reported gap all refuse without binding and without
creating.

`scan_canonical` is deliberately not reused: it is keyed on a
`RuntimeBindingSnapshot` for cursor bookkeeping, and there is no binding here yet
— inventing one would write continuity state for a session that may never be
adopted.

A direct, correlated `agent_created` is unaffected and still requires
`providerOptionsApplied: true`.

## Not claimed

No live authenticating OpenCode seat has been launched through this path from
Kontor. Live Paseo was not restarted, and no seat, workspace or deployment was
created. The upstream E2E is upstream's; this is the caller side.
