# KON-OP-20 — deterministic agent permission posture at spawn (builder evidence)

- **Task:** `01a02a7f-8e47-7682-be52-1b9f2a632ac4`, `ai_short_name: permission posture`.
- **Jira:** none — Kontor-native. **This is not ASMA-7968** (see `README.md` in this directory).
- **Module:** `asma-rs-kontor`, branch `feat/KON-OP-20-permission-posture-at-spawn`, baseline `origin/master` = `e814661`.
- **Plan:** `_docs/ai-orchestration/plans/2026-08-30-15-05-plan-kontor-op20-permission-posture-reconciliation.md` (supersedes the 2026-08-23 plan).
- **Design authority:** `2026-08-22-13-00-design-agent-permission-posture-at-spawn.md` (LSA ASMA-8001 → TPM ASMA-7869 handoff).

## What was wrong

An opencode seat's session mode is not its permission posture. Kontor spawned
seats with `--mode build` and nothing else, so posture fell through to whatever
the machine's harness config carried. On 2026-08-22 twelve of fifteen delivery
seats for the ASMA-8001 catalog epic blocked mid-turn on permission prompts no
human was watching; Kontor recorded them as running and `scheduler-plan` refused
re-admission. Two wedged prompts held an eleven-ticket epic for ~2.5h.

The only thing fixing it at the time was a machine-local edit to
`~/.config/opencode/opencode.json`, which does not travel — a fresh host
reproduces the outage.

## OQ-OP20-1 — answered empirically, not assumed

The plan required the builder to verify on a clean seat that opencode honors a
project-level config before relying on it. Verified against opencode 1.18.15 with
`opencode debug config` (the resolved-configuration readback):

| # | Probe | Result |
| --- | --- | --- |
| A | project `opencode.json` in a non-git dir | honored, deep-merged with global |
| B | the same inside a git repo | honored |
| C | nested dir, config only in an ancestor | honored — opencode walks up |
| D | isolated `HOME`/XDG, **no global config at all** | project config alone resolves exactly as written — the clean-machine case |
| F | root `opencode.json` **and** `.opencode/opencode.json` | both read and merged |
| I | same key in both | `.opencode/` **wins** |
| G | `OPENCODE_CONFIG=<file>` | read, but project config merges *over* it |
| H | `OPENCODE_CONFIG_CONTENT=<json>` | highest precedence of all |

**Answer: yes.** A project-level block is honored, works with no global config
present, and `.opencode/opencode.json` takes precedence over the repository's
root `opencode.json`.

## OQ-OP20-4 (new) — the block is written to `.opencode/opencode.json`

The plan's D3 says `<cwd>/opencode.json`. That target is wrong for the
repositories these seats actually run in, and the deviation is recorded here for
the architect rather than taken silently:

- A seat's cwd is a worktree of the `asma-modules` superproject, which **tracks**
  a root `opencode.json` (model, instructions, mcp). Verified:
  `git ls-files --error-unmatch opencode.json` succeeds.
- Git applies no ignore rule to a tracked file, so D3's stated mitigation —
  `info/exclude` — cannot keep a merge into it out of the seat's own diff.
  Verified: `git check-ignore -v opencode.json` matches nothing.
- Merging there would dirty every seat's worktree and leave Kontor's safety floor
  one `git add` away from being committed as project configuration.

`.opencode/opencode.json` preserves D3's intent exactly — worktree-local,
spawn-time, idempotent, `info/exclude`-hidden, kill-switch respected — while
being untracked (verified) and higher precedence than the committed root file
(probe I). The operator's committed configuration survives untouched.

## OQ-OP20-2 — `auto_accept` remains unwired, deliberately

Re-verified at **Paseo 0.6.1**: `paseo agent run --help` exposes `--mode` and no
`--feature`/`--auto-accept`; `agent update` exposes none either. The live provider
catalogue confirms `auto_accept` exists as a per-agent feature for `opencode` and
`cursor` (and that `claude` and `codex` expose no features at all). Kontor drives
the CLI, not the MCP surface where the feature is settable, so the renderer
derives the intended value and nothing consumes it yet. The permission block is
the guaranteed spawn-time mechanism; the value is derived in the same place as
everything else so a future spawn surface needs no second decision.

## Provider vocabulary — verified, not guessed

Read live from the Paseo provider catalogue rather than assumed, because a wrong
mode spelling is refused at spawn and strands the seat (Paseo 0.4.0 rejecting
`default` for Codex left every replacement verifier permanently queued):

- `claude`: `plan`, `default`, `acceptEdits`, `auto`, `bypassPermissions`
- `codex`: `auto`, `auto-review`, `full-access` — **no read-only mode**, so an
  advisory Codex seat is refused rather than run under a writing one
- `cursor`: `agent`, `plan`, `ask`
- `opencode`: `build`, `plan` only — `build` is described by the provider as
  "Executes tools based on configured permissions", which is the direct
  confirmation that posture belongs in the block, not the mode

## What was built

Status is stated per row: OP-20's deterministic-posture feature is **not
delivered**, because its intended provider is refused (see the disposition
section at the end of this file).

| Plan step | Where | Status |
| --- | --- | --- |
| Abstract `autonomous\|ask\|plan` vocabulary mapped to `SeatAutonomy` | `kontor-daemon/src/runtimes.rs` — `PermissionPosture` + `From` both ways | delivered |
| Versioned migration, back-compatible default | `RUNTIMES_SCHEMA = 5`, `READABLE_SCHEMAS = [4, 5]`; absence resolves to `ask` | delivered |
| Resolution order slot → plane default → `ask` | `applications.rs` `freeze_seat_autonomy` + `RuntimeAdapter::declared_autonomy` | delivered |
| One shared renderer for launch and readback | `posture.rs` — `render_posture`, behind the `seat_posture` gate | delivered |
| Native translation for Claude and Codex | `client.rs` mode tables | delivered |
| Cursor correction — `ask`/`plan` refused, `agent` kept | `client.rs` | delivered |
| Destructive floor, `deny` never `ask` | `DESTRUCTIVE_BASH_DENIES` | delivered (renderer) |
| Bounded per-task override, exact floor keys only | `PaseoTaskSetting.permission_overrides` → `PermissionAllowance` | delivered (renderer) |
| Consultation stays read-only | `SeatPosture::read_only()` at the consultation launch | delivered |
| OpenCode permission block composed at spawn | `seat_mcp.rs` — `<cwd>/.opencode/opencode.json` | **not reachable** — OpenCode delivery is refused |
| OpenCode posture verified before spawn | `seat_mcp.rs` — `verify_composed_posture` | **not sound** — see the disposition; retained for the re-enabled path |
| `auto_accept` where the spawn surface exposes it | derived in `SeatPosture`, unconsumed | **not wired** — no Paseo CLI surface (OQ-OP20-2) |

The resolution order is slot → plane default → `ask`, so a template that already
declared a seat's autonomy is never overruled by a plane-wide default.

## Out of scope, untouched

- `launch_hosted_seat_inner` (ECP leadership seats) stays `Supervised`.
- The machine-local `~/.config/opencode/opencode.json` stopgap is documented, not
  redone, and is never written by this composition.
- The superproject gitlink is not advanced here.

---

# Post-review revision (2026-08-30, after inspector BLOCKED verdict)

The inspector reviewed `4c93739` and blocked on two findings. Both are fixed in
`53dba77`; the non-blocking findings are recorded here rather than closed.

## F5 (blocking) — FIXED · a bounded override that was not an exact floor key
defeated the floor

`PermissionAllowance::parse` refused only blank and all-`*` strings, so `*git*`
was accepted as a "named" pattern. Under the evaluation mechanism now verified
(below), it renders *after* both git denies and is evaluated last — so
`"permission_overrides": ["*git*"]` would silently delete the git half of the
floor from one line that never spells `*`. `*rm*` and `*submodule*` defeat their
families identically. The type existed precisely to make the bounded override
safe by construction and did not achieve it.

**Fix.** An allowance must be character-for-character a member of
`DESTRUCTIVE_BASH_DENIES`. The allowance set is then a subset of the floor by
construction: an override can only flip a deny that already exists, in the
position it already occupies, and can never introduce a new later-sorting rule.
Broad, near-miss, prefix, suffix, case-variant, unknown, blank and wildcard
patterns are refused at the type boundary and again at fleet composition, before
any plane is composed and therefore before any effect. A key-set invariant test
(`an_allowance_can_only_flip_a_floor_key_never_add_one`) pins the structural
property that makes ordering irrelevant.

## F1 (blocking) — FIXED · the floor's membership had no independent oracle

Every floor assertion looped over `DESTRUCTIVE_BASH_DENIES`, so deleting a member
merely stopped checking it. The inspector's sweep showed four of five removals
left the suite green.

**Fix.** `the_floor_is_exactly_the_five_published_patterns` asserts the exact
five-element list and its cardinality as literals;
`every_published_pattern_is_denied_by_literal_name` asserts each pattern is denied
by name in the rendered block under both writing postures. A 10-mutant sweep
(remove each member, respell each member) now kills all ten; see `MUTATION.md`.

**Correction.** The previous revision of `MUTATION.md` claimed "9 of 9 mutants
killed". That overstated the floor's defence — none of those nine changed the
floor's membership. The claim is corrected there.

## The evaluator assumption is closed, with the mechanism corrected

The earlier revision recorded as an open assumption that a specific `deny` beats
`"*": "allow"`. Verified here against the installed OpenCode 1.18.15 binary:
`fromConfig` walks `Object.entries` outer and nested (preserving the serialized
key order), `evaluate` resolves with `.findLast` (last match wins), and an
unmatched tool defaults to `ask`.

**The ordering is lexicographic, not insertion order.** `serde_json` is pinned at
`=1.0.151` and its lock entry pulls in no `indexmap`, so `preserve_order` is off
and `serde_json::Map` is a `BTreeMap`. `"*"` is a prefix of every floor pattern
and therefore sorts before all of them, which is what lets the denials win.
Recording this as insertion order would be the reasoning that later justifies
appending a non-floor allowance — the F5 defect.

## F2 (non-blocking) — DOCUMENTED, not fixed

OpenCode merges rather than replaces, and the `ask` block names only `bash` by
design. On a host still carrying the machine-local 2026-08-22 stopgap, every other
tool resolves from that ambient config, so an `ask` seat still edits files without
asking. On a clean host the unlisted tools fall to OpenCode's own default, which
its evaluator gives as `ask`. The code is correct as designed; the missing
sentence was the operational precondition, now stated in `CONFIGURATION.md`.
Removing the stopgap is out of scope for this task.

## F3 (non-blocking, latent) — RETAINED as a reviewed observation

`cursor` gained delivery postures here, while `consultation_permission_mode`
refuses cursor thirty lines away on an attested finding that its ACP runtime
permits shell writes in both modes. An advisory cursor seat would therefore carry
a containment claim with no mechanism behind it. **Latent, not live:** the
governed model catalog exposes no cursor provider, so no route can select it
today — the guard is the catalog, not this layer. Not changed here, because
refusing cursor's `Advisory` arm is a design decision belonging to the architect,
and the reconciled plan explicitly put Cursor translation *in* scope. Recorded so
it is decided before cursor is ever catalogued; **not verified away.**

## F4 (non-blocking, observation) — RETAINED

`verify_agent_route_with_mode` compares `current_mode_id` only, and for opencode
both `autonomous` and `ask` spell `build`. Readback therefore cannot distinguish
the two postures, and the permission block is never re-read after launch. An
inherent limit of verifying through Paseo rather than a defect introduced here.
Carried as OQ-OP20-6. **Not verified away.**

---

# Disposition: fail closed on OpenCode delivery (2026-08-30)

**OP-20 remains in progress. This is an interim safe state, not acceptance of
the ticket, and not grounds to close the task or the epic.**

## Why the original approach could not be finished

Verification of an OpenCode seat's posture was attempted by reimplementing
OpenCode's configuration resolution. Successive review rounds each found another
input: the machine-global config, the repository's root `opencode.json`, both
`opencode.jsonc` siblings — and finally three environment variables:

| Input | Effect |
| --- | --- |
| `OPENCODE_CONFIG_CONTENT` | injects a whole configuration inline, outranking project files |
| `OPENCODE_PERMISSION` | injects a permission block directly |
| `OPENCODE_DISABLE_PROJECT_CONFIG` | makes OpenCode ignore the composed file entirely |

All three names are embedded in the installed 1.18.15 binary. The decisive point
is not the count but the location: they are read by the **spawned** process,
which Paseo creates. `paseo agent run` exposes no way to set or read that
process's environment (verified against Paseo 0.6.1). So reading configuration
files from the daemon cannot establish what the seat resolves, and running
`opencode debug config` from the daemon resolves the *daemon's* environment,
which is a different question. The layered merge was unsound in principle rather
than merely incomplete.

## What was done instead

`seat_posture` refuses an OpenCode delivery launch with
`PermissionModeUnsupported`, resolved before any transport call, so a refusal
spends no census, no placement lookup and no spawn. The translation moved to
`render_posture` and stays complete and under test.

## The dependency this lifts on

Paseo must expose **either** an attested resolved configuration per agent,
**or** a seat process environment Kontor can set and verify. Until one exists,
no OpenCode delivery posture can be claimed as deterministic.

## Deferred, and still required if OpenCode delivery is re-enabled

Two findings raised against the earlier design were **not** built, deliberately,
because both sit on top of the verifier whose soundness failed. They do not
affect the fail-closed path — no OpenCode seat launches, so neither can occur —
and both become blocking again the moment delivery is re-enabled:

- **Shared-file race.** A TeamRun can launch several OpenCode role seats into one
  task worktree while posture resolution permits per-role-slot differences. They
  would share a single `.opencode/opencode.json`, and a later seat would rewrite
  the first seat's effective policy. Requires either true per-seat config
  isolation in the spawned environment, or a validated task-wide identical
  posture invariant with the file immutable while holders exist.
- **Post-spawn compensation.** The second readback runs after `agent run`. On
  drift it returned a refusal before bind, which would leave a created native
  agent untracked. Requires a governed compensating archive correlated to the
  exact native id, a terminal-state readback, and a receipt-bearing failure —
  with a typed needs-human outcome retaining the binding when compensation
  cannot be proved.

## Retained from the earlier rounds

F3 (Cursor) is now **fixed** rather than observed: `ask` and `plan` are refused.
F4 (readback cannot see an OpenCode posture through the mode) is subsumed by the
delivery refusal. F2's ambient-global leak no longer governs a Kontor-launched
seat, because none launches.

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

## OQ-OP20-5 resolved, and what now blocks the dispatch — 2026-08-31

The prior inspector could not evidence an inspector agent run for this task and
declined to settle rather than guess one. That run exists and is now named.

**The inspector run is `01a0306f-0816-7ab3-a790-036a6ef11cdc`.** It was found on
the native agent's own labels (`kontor.agent-run`) and confirmed by
`kontor_run_get`: role `inspector`, team run
`01a0306e-6de5-7bb2-a8b6-18a8dd01bbb9`, bound to Paseo agent
`10fd5152-c9b9-499d-b9ec-1cec2876901b`, which is alive and idle in the OP-20
worktree.

Two corrections to OQ-OP20-5's reasoning, both mine to state because I ran the
calls:

* `01a0306e-8fda-75d2-9f47-5ce1add8016b` is not a Core Team seat binding. It is
  the **TeamRun delivery** seat binding for `role_slot_id: inspector` on the
  OP-20 TSW node. The server says so itself — `kontor_topology_seat_message_send`
  refuses it with *"TeamRun delivery seats are messaged through the session
  message surface"*. That surface is `kontor_session_message_send`, which keys on
  the agent run, not the seat binding. The namespaces are indeed different; the
  conclusion drawn from that was the wrong one.
* `paseo ls` listing no OP-20 agent was a stale or mis-scoped read.
  `list_agents` scoped to this worktree returns all five seats of the team run.

The idempotency key on both surfaces must be a **UUIDv7**, not free text and not
a v4 UUID; the handler parses it as a `MessageId`. A non-v7 key is refused with
`invalid_request` / *"the identifier is not in canonical form"*, which names no
field and is easy to misread as being about the path ids.

### What blocks it now

`kontor_session_message_send` refuses with **409 `stale_binding`** — *"this
process holds no frozen capability snapshot for the session"*. The run reports
`binding.attached: false`, `projection.freshness: stale`, last confirmed
2026-08-23 at cursor 904 against snapshot cursor 1591, `gaps: []`,
`closed_at: null`, `lifecycle: running`. This is
`operational-gap-asma-7869-stale-runtime-binding-unsettleable-20260830`, which
the plan's §5 puts outside this task's scope.

The refusal names `kontor_runtime_settle` as the next action. **It was not
called.** Settling resolves the run against the runtime's own verdict, and Paseo
reports this agent `idle` with `attentionReason: finished` — so a settle may
close the run rather than refresh it, and a closed inspector run cannot be
dispatched without a replacement seat, which the standing instruction forbids.
That is an operator decision, not a builder one.

The plan's §5 names the other route: the bounded direct-prompt fallback, which
prompts the existing native agent through Paseo and reconciles through Kontor
once the binding is settleable. It creates no session and no workspace. It is
also not "through the recorded topology", so it is not taken unasked either.

Builder acceptance at the time of the attempt: paseo lib 107, paseo contract
161, daemon lib 57, daemon integration 232, clippy 0, fmt clean, both trees
clean, mutation 9/9 + 5/5.

### Dispatch taken — 2026-08-31

The operator chose the bounded direct-prompt fallback. The existing inspector
seat — native agent `10fd5152-c9b9-499d-b9ec-1cec2876901b`, run
`01a0306f-0816-7ab3-a790-036a6ef11cdc` — was prompted through Paseo and accepted
the turn. No session or workspace was created, no run state was written, and
`kontor_runtime_settle` was still not called.

The handoff tells the inspector how it was reached, so its own close-out does not
mistake this for a governed message: its turn settlement will hit the same
`stale_binding` refusal, and it is asked to record that rather than work around
it. Reconciliation through Kontor waits on the ASMA-7869 gap.

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
