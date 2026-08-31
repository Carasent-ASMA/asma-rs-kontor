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

One `create_agent_request` is the whole launch. It carries
`config.providerOptions.permission`, the typed MCP surface in
`config.mcpServers`, `config.initialPrompt`, a `config.clientMessageId` derived
from the launch rather than generated, and a launch-intent digest label over the
whole configuration **and** the prompt and its message id.

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
- **Mutation: 20/20 killed** over the caller gates. Three survived their first
  run, and each survival was the finding:
  - an incomplete census treated as complete stayed green because the test
    asserted only the error *variant*, and "could not enumerate" and "found none"
    share it. The assertion now names the rule.
  - adopting an already-owned match, and adopting one of several, both stayed
    green because neither quarantine branch had a test at all.
  - a digest whose fields run together without delimiters still changed when the
    prompt changed, so the existing assertions could not see the concatenation
    collision. One now can.

## Not claimed

No live authenticating OpenCode seat has been launched through this path from
Kontor. Live Paseo was not restarted, and no seat, workspace or deployment was
created. The upstream E2E is upstream's; this is the caller side.
