# OP-20 — OpenCode delivery re-enabled on an applied-policy acknowledgement

- **Task:** `01a02a7f-8e47-7682-be52-1b9f2a632ac4`, "Deterministic agent
  permission posture at spawn time". Not ASMA-7968.
- **Supersedes:** every fail-closed disposition in this directory, and the
  `2026-08-31-upstream-dependency-applied-permission.md` note, whose dependency
  is now satisfied.
- **Upstream:** ASMA-7869, exact-v0.6.1 backport
  `a8781451415c065910cc768999a1129222e7204a` (parent exact tag `20d7efc…`),
  green and packaged-smoke proven. It applies the exact ordered OpenCode policy
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
unbound match adopts, one already-bound match refuses, none on a *complete*
enumeration is `DeliveryConfirmationUnknown`, and several — or an enumeration
that did not finish — is quarantined.

`DeliveryConfirmationUnknown` no longer releases the seat claim. Releasing it is
what would licence a second create for the same run. Only the daemon's own
`agent_create_failed` releases, because only that states nothing was made.

A created seat that fails any check is archived and read back terminal; an
unconfirmable archive refuses recoverably. A durable bind failure returns
confirmation-unknown, keeping the claim and the intent label so reconciliation
adopts that very agent.

Claude, Codex and Cursor keep the CLI create and readback unchanged.

## Acceptance

- fmt clean; `clippy --workspace --all-targets -D warnings` 0; rustdoc
  broken-intra-doc-link gate for `kontor-runtime-paseo` exit 0.
- `kontor-runtime-paseo` lib 92, contract 166; `kontor-daemon` lib 57.
- **Mutation: 30/30 killed** over the caller gates — twenty on the launch
  wiring, two on the create envelope once it was corrected (a wrong declared
  response type, and `initialPrompt` moved off the message top level), and eight
  on the two create-to-bind findings below. Three survived their first
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

**`agent_create_failed` is not evidence that nothing was created.**
`resolveSessionCreateAgent` sets `promptFailure: "throw"`, and
`createAgentCommand` creates the agent *before* it sends the initial prompt. A
prompt that fails therefore throws out of the command, `createdAgentId =
snapshot.id` is never reached, and the catch emits `agent_create_failed` while
the agent is running — an agent the daemon's own
`cleanupCreatedWorktreeAfterFailedAgentCreate` also skips, having been handed a
null id. So it releases nothing and goes to the census like every other ambiguous
outcome.

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
