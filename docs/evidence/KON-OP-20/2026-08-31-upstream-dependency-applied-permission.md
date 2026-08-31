# OP-20 upstream dependency — an applied OpenCode permission policy

- **Task:** `01a02a7f-8e47-7682-be52-1b9f2a632ac4`, "Deterministic agent
  permission posture at spawn time". Not ASMA-7968.
- **Status:** OP-20 is **in progress and not delivered.** OpenCode delivery is
  fail-closed. Delivery waits on the surface described here.
- **Source:** inspector verdict BLOCKED, turn
  `01a054f9-7e66-7e71-ae96-b10f26cda005`, finding B1; independently confirmed by
  the operator.

## What is missing

Paseo 0.6.1 offers **no carrier that delivers an OpenCode permission block to
the spawned process, and no acknowledgement that one was applied.** Every route
Kontor has tried fails for its own reason.

| Route | Why it fails |
| --- | --- |
| Worktree `opencode.json` / `.opencode/opencode.jsonc` | The project layer is discarded by `OPENCODE_DISABLE_PROJECT_CONFIG`, and the active-org remote config and managed profiles merge after it. |
| `OPENCODE_CONFIG`, `OPENCODE_CONFIG_DIR`, an owned `XDG_CONFIG_HOME` | Same merge order, plus `OPENCODE_CONFIG_CONTENT` and `OPENCODE_PERMISSION` inject permissions above all of them. `agent run` sets no per-agent environment. |
| `create_agent_request` → `config.providerOptions.permission` | **Inert.** The daemon validates and persists it, and it never reaches a seat. |

### Why `providerOptions` is inert, from the installed bundles

1. Paseo imports `@opencode-ai/sdk/v2/client`. Its `promptAsync` allow-lists the
   body keys `messageID, model, agent, noReply, tools, format, system, variant,
   parts`, with no `allowExtra`; `buildClientParams` silently discards anything
   unmapped. The permission is computed and passed correctly and never leaves
   the daemon process.
2. OpenCode 1.18.15's `SessionPrompt.prompt` builds its session rules from
   `Object.entries(t.tools)` only. That route reads no `t.permission`.

No other route carries it either: `session.create({directory})` passes none,
`session.update` is used only for the archive timestamp, and `server-manager.js`
has zero occurrences.

The consequence: a seat created this way runs under whatever OpenCode's own
layers resolve, which on a clean host is the evaluator's `ask` default for every
unmatched tool — the 2026-08-22 wedging this task exists to prevent. An accepted
create and an accepted first turn both prove the daemon parsed the request. They
say nothing about the policy the process applied.

## What Paseo must expose before OP-20 can be delivered

**The mechanism already exists on adjacent routes.** The v2 SDK allow-lists
`permission` on `session.create` and `session.update`, and OpenCode's
`SessionHttpApi.update` installs it through `setPermission`. What is missing is
Paseo using them and saying so.

1. **Attach the typed permission at session create or update** — not per prompt.
2. **Return a correlated applied-policy acknowledgement**: a
   `providerOptionsApplied` / applied-policy field naming the request and the
   agent, **or** an effective-permission readback a launch can compare against
   the block it sent. Acceptance of a turn is not this. It must be an assertion
   about the process, not about the frame.

Only then can a launch bind on evidence rather than on parsing.

## What the eventual launch will also need

These were built once and deleted with the two-stage path, because none of them
is sound without the acknowledgement above. They are listed so the work is not
rediscovered:

- **An intent digest over the create *and* the prompt.** The deleted version
  covered the create only, so the same ids with different prompt text produced
  an identical digest and an identical message id — and a stale turn satisfied
  the proof for different content.
- **A durable create-to-bind saga.** Between a confirmed create and a completed
  bind there are several fallible steps, and a failure in any of them strands a
  seat that is running and owned by nobody. It must survive a restart, which
  the deleted version could not: every adapter is built from
  `PaseoCheckpoint::fresh`.
- **A census-zero outcome that is not `Transport`.** `launch` releases the
  admission on every error, so a retry after an ambiguous create could create a
  second seat. `DeliveryConfirmationUnknown` already exists and is what the AO
  adapter uses for this hazard.
- **A multi-page confirmation scan.** The deleted first-turn proof read one
  page; `scan_canonical` already documents the counterexample.
- **Compensation on every post-create failure**, not only on a refused first
  turn.

## Not claimed

No live authenticating OpenCode seat has been launched through any of this. The
findings above are read from installed bundles and from the code, not from a
running seat.

---

# CURRENT DISPOSITION — 2026-08-31 (third revision; supersedes everything above)

**OpenCode delivery is re-enabled**, gated on the daemon advertising
`providerOptionsApplied` and on an explicit per-agent `providerOptionsApplied:
true` on the correlated `agent_created` snapshot. Upstream ASMA-7869 now applies
the ordered policy at OpenCode `session.create`/`session.update`.

Both earlier dispositions in this file are superseded: the two-stage
`providerOptions`-on-the-create path (which never applied) and the fail-closed
refusal that replaced it. What ships is one create carrying the permission, the
MCP surface, the prompt and a derived client message id, bound only on the
per-agent acknowledgement.

See `2026-08-31-delivery-re-enabled-on-applied-acknowledgement.md`. Anything
above describing an owned configuration root, a seat environment, a written
block, a separate first-turn proof, or an unconditional refusal is **history**.
