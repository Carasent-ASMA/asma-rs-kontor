# KON-OP-20 — permission-posture mutation proof

Date: 2026-08-30 (revised after inspector review)
Task: `01a02a7f-8e47-7682-be52-1b9f2a632ac4` (permission posture). **Not ASMA-7968.**
Branch: `feat/KON-OP-20-permission-posture-at-spawn`, baseline `origin/master` `e814661`.
Sweep run at `53dba77`.

## Correction to the previous revision of this file

The earlier revision claimed **"9 of 9 mutants killed"**. That overstated what the
floor was defended against, and the inspector was right to say so. None of those
nine mutants changed the floor's *membership*; they changed its *value*
(`deny` → `ask`), which the self-referential loops did catch. A deterministic
sweep confirmed that removing four of the five floor patterns left the suite
**green**. That gap is now closed by literal membership assertions, and the sweep
below includes the membership mutants that exposed it.

The earlier revision also left the OpenCode evaluator as an open assumption. It is
now verified, and one piece of the reasoning is corrected: **the ordering is not
insertion order.**

## The evaluation mechanism, verified at the exact installed version

Extracted from the Bun-compiled `/opt/homebrew/Cellar/opencode/1.18.15/bin/opencode`,
which embeds its JS verbatim:

```js
// evaluate — last match wins, unmatched tools default to ask
function c(j,J,...K){
  return K.flat().findLast((z)=>g.match(j,z.permission)&&g.match(J,z.pattern))
      ?? {action:"ask",permission:j,pattern:"*"}
}
// fromConfig — walks Object.entries for the outer map and each nested one
function RA(j){let J=[];for(let[K,z]of Object.entries(j)){ ... }}
```

Four facts, each checked here rather than assumed:

1. `fromConfig` iterates `Object.entries` outer **and** nested, so the rule list
   preserves the key order of the file as serialized.
2. `evaluate` resolves with **`.findLast`** — the *last* matching rule wins.
3. An unmatched tool defaults to `{action:"ask"}`.
4. The order is **lexicographic**, not insertion order. This workspace pins
   `serde_json = "=1.0.151"`, and its `Cargo.lock` entry depends only on
   `itoa, memchr, serde, serde_core, zmij` — no `indexmap`, so `preserve_order` is
   off and `serde_json::Map` is a `BTreeMap`.

`"*"` is a prefix of every floor pattern, so it sorts **before** all of them and
the specific denials are evaluated after the catch-all and win. Stating this as
insertion order would be wrong, and would be exactly the reasoning that later
justifies appending a non-floor allowance — which is the F5 defect.

## The sweep

Each mutation was applied to the corrected source, **verified to have landed**
(`git diff --quiet` on the mutated file), the named tests were run against that
broken build, and the mutation was reverted. A mutant producing a *compile* error
rather than a test failure counts as survived. Floor mutants are edited inside the
constant's own block, so they cannot accidentally hit a test literal instead.

Run with an isolated `CARGO_TARGET_DIR`; the tree was verified clean afterwards.

### F1 — the floor's membership (the gap the inspector found)

| # | Mutation | Result |
| --- | --- | --- |
| F1-remove ×5 | delete each of the five patterns from `DESTRUCTIVE_BASH_DENIES` | **all 5 killed** |
| F1-drift ×5 | respell each pattern (`*rm -rf *`→`*rm -rf*`, `*git clean -*`→`*git clean*`, `*git rm --cached*`→`*git rm --cache*`, `*submodule update*`→`*submodule updates*`, `*submodule deinit*`→`*submodule de-init*`) | **all 5 killed** |

Killed by `the_floor_is_exactly_the_five_published_patterns` and
`every_published_pattern_is_denied_by_literal_name`, which name all five patterns
as literals and so cannot be silenced by editing the constant.

### F5 — the bounded override must be an exact floor key

| # | Mutation | Result |
| --- | --- | --- |
| F5-lax | `parse` accepts any non-blank pattern (the reviewed defect) | **killed** |
| F5-lax | the same defect, checked at the fleet-composition boundary | **killed** |
| F5-case | `parse` matches a floor pattern case-insensitively | **killed** |
| F5-prefix | `parse` accepts a prefix of a floor pattern | **killed** |

### The original gates, re-checked on the corrected source

| # | Mutation | Result |
| --- | --- | --- |
| M1 | `READABLE_SCHEMAS` drops generation 4 | killed |
| M2 | resolution never consults the plane default | killed |
| M3 | the plane default overrules the role slot | killed |
| M4 | the floor renders `ask` instead of `deny` | killed |
| M6 | the `ask` posture also spells `read: ask` | killed |
| M7 | a task exception leaks into the launch mode | killed |
| M8 | Cursor's `ask` renders as `agent` | killed |

**21 of 21 mutants killed.**

## Superseded scope note (2026-08-30, fail-closed disposition)

OpenCode delivery is now refused before any native call, so the mutants below
that exercise the OpenCode composition and readback describe the **re-enabled**
path rather than what runs today. They are retained because the code they test is
retained. The gate itself carries its own mutation evidence: removing the refusal
in `seat_posture` turns both `opencode_delivery_is_refused_until_it_can_be_proved`
and the launch-boundary test
`an_opencode_delivery_launch_is_refused_before_any_native_call` red.

## What this still does not prove

- **Readback cannot see the posture on OpenCode** (inspector F4). Both
  `autonomous` and `ask` spell `build`, and the permission block — the only
  carrier of the difference — is never re-read after launch. A
  `.opencode/opencode.json` deleted or edited post-spawn passes verification.
  This is an inherent limit of verifying through Paseo, not something this change
  introduced, and it is retained as a reviewed observation rather than fixed here.
- **No live autonomous seat was run on a clean host.** The evaluator semantics are
  verified by extraction and the rendering by test; an end-to-end seat that
  provably never blocks has not been observed.
- **Cursor's delivery postures are latent** (inspector F3). Cursor is now in the
  mode tables but the governed model catalog exposes no cursor provider, so no
  route can select it. The containment claim behind an advisory cursor seat rests
  on the catalog, not on this layer.

---

# CURRENT DISPOSITION — 2026-08-30 (supersedes every status above)

**OpenCode delivery is fail-closed.** `preflight::attest_spawn_environment`
refuses every OpenCode delivery launch, before the capability read, the provider
diagnostic, the owned root and the preflight — so nothing native is read or
created.

**Why.** Two inputs to an OpenCode seat's permission are decided outside
everything Kontor pins: the auth-backed **active-org remote configuration**,
which follows whichever credentials the spawned process resolves, and the
**system managed layer**, read at process start. Both sort *after* the owned
configuration, so both can state a permission the preflight never saw. Whose
credentials the spawned process uses depends on the inherited `HOME` /
`XDG_DATA_HOME`, and `paseo provider diagnostic` reports neither — it prints
`Daemon PATH`, a `Daemon shell`, and an *unexpanded*
`~/.local/share/opencode/auth.json`. No Paseo call reports the configuration a
created agent actually resolved.

**Correcting an earlier claim in this file.** The owned root does **not** remove
the active-org or managed layers. It displaces the user global and every project
layer; the later two are only *observed* at preflight time by full-object
comparison, and observation before creation is not proof about the process that
is created afterwards.

**What is built and stays under test**, waiting on that dependency: the
`autonomous|ask|plan` vocabulary, schema v5 and resolution order, the shared
renderer and destructive floor, the exact-floor allowance rule, the Cursor
correction, the owned per-seat configuration root with pre-existing-link refusal, the
typed six-key `agent run --env` surface with redaction, the daemon version pin,
the posture digest for lost-acknowledgement recovery, and the installed-binary
preflight.

**Lifts when** Paseo can attest either the spawned process's environment or the
configuration a created agent resolved.

## Unresolved prerequisite — configuration-root races

`SeatConfigRoot::materialize` refuses path components that are **already**
symlinks, and stages-then-renames so a pre-existing link is never written
through. It is **not race-safe**, and no evidence here should be read as saying
it is: every check and every open addresses by pathname, so a writer running as
the same Unix user — which an OpenCode seat does — could replace a checked
directory between the check and the open.

Closing it needs fd-relative traversal throughout (`openat`/`O_NOFOLLOW`, rename
anchored to a directory descriptor) with a deterministic race or failpoint test.
That is **a prerequisite for re-enabling OpenCode delivery**, not something the
current fail-closed state depends on: no seat reaches this code today.

---

# CURRENT DISPOSITION — 2026-08-31 (supersedes every status above)

> **RETRACTED 2026-08-31.** This section is false and is kept only as a record
> of what was tried. `providerOptions.permission` never reaches a seat — Paseo's
> v2-SDK `promptAsync` allow-lists its body keys and drops it, and OpenCode's
> `SessionPrompt.prompt` reads only `t.tools`. OpenCode delivery is fail-closed
> and the two-stage path is deleted. See the second-revision disposition at the
> end of this file.

**OpenCode delivery is reachable, in two proved stages, and is no longer
fail-closed.** Every earlier disposition in this file — the environment
approach, the owned configuration root, the spawn-environment attestation and
the refusal that rested on it — is superseded and the code implementing them is
deleted.

## What ships

`launch_admitted` branches for OpenCode alone; every other provider keeps the
CLI create it had, and Claude's worktree MCP composition is untouched.

1. Gates before any native call: the daemon must accept typed per-agent
   `providerOptions`, and the provider must express the declared posture.
2. `create_agent_request` with the rendered permission in
   `config.providerOptions.permission`, the MCP surface in `config.mcpServers`,
   a launch-intent digest label over the whole create configuration, and **no**
   `initialPrompt`.
3. A lost acknowledgement reconciles by exact-label census — one adopted, none
   confirmation-unknown, more than one refused — and the create is never resent.
4. The first real turn, with a message id derived from the launch; the daemon
   replays the persisted `providerOptions.permission` into `session.promptAsync`
   and OpenCode installs it before evaluating a tool call.
5. Binding only on an answer that names that request and that agent and says
   accepted. Otherwise: archive over the same socket, read back terminal, and a
   recoverable refusal if that cannot be confirmed.

## Why this is sound where the earlier approaches were not

The policy never passes through a file or an environment variable, so the merge
order that defeated every earlier design — global, project, `.jsonc` siblings,
`OPENCODE_*` variables, active-org remote config, managed profiles — has nothing
to act on. Acceptance of the turn is the acknowledgement, and it is about the
process that ran it.

## Not claimed

No live authenticating seat has been launched through this path. That is a
post-integration, post-deployment proof and is not asserted here.

## Two-stage delivery mutation sweep — 2026-08-31 (9/9 killed)

Against the committed path, each mutant verified to have landed and reverted
after; tree verified clean.

| Mutation | Result |
| --- | --- |
| gate accepts any daemon, with no providerOptions contract | killed |
| create is resent after a lost acknowledgement | killed |
| the first-turn proof is skipped entirely | killed |
| a failed first turn is not compensated | killed |
| acceptance trusts the boolean and ignores correlation | killed |
| compensation reports archived without reading it back | killed |
| the create carries an `initialPrompt` after all | killed |
| `providerOptions` is dropped from the create | killed |
| the launch intent digests identity only, not the config | killed |

The compensation mutant survived the first sweep: the existing test asserted the
archive was *sent*, not that it read back terminal, so removing the confirmation
left it green. `an_unconfirmed_archive_refuses_recoverably` closes that.

## Boundary sweep — 2026-08-31 (5/5 killed)

The two cases the delivery sweep left unproved: a lost first-turn
acknowledgement, and two seats sharing one worktree.

| Mutation | Result |
| --- | --- |
| a lost first-turn ack resends the prompt instead of reading the timeline | killed |
| the timeline scan affirms without looking for the message id | killed |
| an OpenCode seat writes a config file into the worktree it shares | killed |
| the pre-create census keys on the team run rather than the role slot | killed |
| adoption ignores the launch intent and takes any live agent | killed |

The fourth survived its first run, and the reason is the finding: the
recorded daemon answered every census empty, so the second seat never saw
the first and the collision the test claimed to rule out could not occur.
Making the census report the seat that just launched turned the mutant into
exactly the 2026-08-22 failure — `SlotAlreadyAdmitted` on a neighbour that
shares nothing but a worktree.

---

# CURRENT DISPOSITION — 2026-08-31 (second revision; supersedes everything above)

**OP-20 is in progress and is not delivered. OpenCode delivery is fail-closed.**

The two-stage `providerOptions` path described in the 2026-08-31 disposition
above **did not ship and has been deleted.** Its load-bearing claim — that the
daemon persists the permission and replays it into every turn — is false:
Paseo`\s v2-SDK `promptAsync` allow-lists its body keys and drops `permission`,
and OpenCode 1.18.15`\s `SessionPrompt.prompt` reads only `t.tools`. The field
is validated, persisted, and never reaches a seat.

So every earlier disposition in this file is superseded, including the one that
said the path had shipped. An OpenCode delivery launch is now refused before any
transport call, native effect or worktree write.

Source: inspector verdict BLOCKED, turn
`01a054f9-7e66-7e71-ae96-b10f26cda005`, finding B1, confirmed by the operator.
The blocking dependency is recorded in
`2026-08-31-upstream-dependency-applied-permission.md`.

Anything in this file describing a written block, an owned configuration root, a
seat environment, a launch-intent digest, a first-turn proof or a create-to-bind
compensation is **research, not delivery**. It is retained because it records
what was tried and why it failed, not because it runs.

## Fail-closed gate sweep — 2026-08-31 (4/4 killed)

After B1, the only OpenCode delivery assertion left is that the launch is
refused before anything native happens. All four mutants of that gate are
caught by `an_opencode_delivery_launch_is_refused_with_no_native_effect`.

| Mutation | Result |
| --- | --- |
| the gate call is deleted from `launch_admitted` | killed |
| the gate stays but returns `Ok(())` for OpenCode | killed |
| the refusal moves after the first native read | killed |
| the refusal is typed as something other than the posture refusal | killed |

The third matters as much as the first: a gate that refuses *after* a workspace
read has already spent a native call, and the test measures effects, not just
the error.

The nine-mutant delivery sweep and the five-mutant boundary sweep recorded above
are **void**. They exercised the two-stage path, which is deleted; several of
them were assertions about guarantees that never held, and the inspector's own
generalisation of the census-fixture defect applies to at least three
(`a_lost_create_acknowledgement_is_never_answered_by_a_second_create` invoked
`launch` once and asserted a create count of one, which one invocation cannot
falsify). They are left in this file as a record of what was tried.

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

## Re-enabled delivery sweep — 2026-08-31 (20/20 killed)

Against the committed one-stage path. Each mutant verified to have landed at the
intended site and reverted after; tree verified clean.

| Mutation | Result |
| --- | --- |
| the capability gate accepts any daemon | killed |
| the gate falls back to a version floor | killed |
| binding without the per-agent acknowledgement | killed |
| a `false` acknowledgement is treated as applied | killed |
| a missing acknowledgement is treated as applied | killed |
| the create is resent after a lost answer | killed |
| census-zero is a plain failure, so the claim is released | killed |
| the unresolved-create guard is removed from the release rule | killed |
| an incomplete census is treated as complete | killed |
| the census stops paginating and calls itself complete | killed |
| an invalid created native is not compensated | killed |
| compensation reports archived without reading it back | killed |
| a durable bind failure is reported plainly, stranding the seat | killed |
| the create drops `providerOptions` | killed |
| the create drops `initialPrompt` | killed |
| the client message id is not derived from the launch | killed |
| the launch intent's fields run together without delimiters | killed |
| a post-ack fetch replaces the create snapshot | killed |
| an already-bound match is adopted anyway | killed |
| several matches are adopted rather than quarantined | killed |

Three survived their first run. Each survival was the finding, not the mutant:

1. **Incomplete census.** The test asserted only
   `RuntimeError::DeliveryConfirmationUnknown`, and both "the enumeration could
   not finish" and "the enumeration found none" are that variant. A census that
   stopped paginating and called itself complete was therefore invisible. The
   assertion now names the rule text.
2. **Two quarantine branches with no test.** Adopting a match another run already
   owns, and adopting one of several matches, were both unexercised — the
   mutants could not fail a test that did not exist.
3. **Digest delimiters.** Removing `prompt=` and `client_message_id=` still made
   the digest change when the prompt changed, so every existing assertion stayed
   green while a concatenation collision became possible. A case that moves one
   character across the boundary catches it.

An earlier sweep also recorded a mutant that appeared to survive because the
edit landed on the wrong `if !complete` — there are three in this file. Mutants
are now anchored on enough surrounding text to be unambiguous, and the harness
reports `NOT-APPLIED` rather than `SURVIVED` when its anchor is missing.

### Envelope pins — 2026-08-31 (2/2 killed)

| Mutation | Result |
| --- | --- |
| the create declares response type `create_agent_response` | killed |
| `initialPrompt` moves off the message top level | killed |

Both are wire details no fixture can falsify: the recorded transport answers
with whatever response type the request declared, and nothing else asserts where
the daemon reads the prompt from. They are pinned against
`PaseoRpc::hosted_seat_agent_create`, which is evidenced in production, rather
than against a fixture.
