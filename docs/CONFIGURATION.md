# Configuration

Kontor separates invariants from deployment behavior. Rust enforces safety
properties such as one non-terminal session per role slot and uncertainty not
being completion. Names, durations, prompts, skills, profiles, teams, roles,
topology, committees, completion, budgets and runtime routing are versioned data.

That split is the point, not an implementation detail: the workflow being data is
what lets Kontor run research, architecture, UX, QA and operations work without a
core-code branch per work type, and what lets an operator's own conventions become
system behaviour instead of instructions somebody has to remember.

## Where configuration lives

| Location | Holds |
| --- | --- |
| `<state-root>/kontor.db` | Every versioned specification published through `/v1`: topology specs, role catalogs, work profiles, team templates, advisor profiles, committee templates, completion profiles, Core Team revisions, connector field/workflow specs |
| `<state-root>/runtimes.json` | Runtime family, plane endpoint, per-account provider aliases and the plane's default seat posture. Schema generation `5`; generation `4` is read as a `5` that declares no posture, which resolves to `ask`; generation `3` is refused rather than upgraded, because it can compose the right sessions under misleading names |
| `<state-root>/supervision.yml` | Seat supervision policy (optional; see below) |
| `<state-root>/credentials.json` | The realm's three tier secrets, `0600` |
| `<state-root>/endpoint.json` | Where the realm listens, when not on the default loopback port |
| `<state-root>/provider-homes/` | One credential home per provider account — `CODEX_HOME` for Codex, `CLAUDE_CONFIG_DIR` for Claude |
| `crates/kontor-mcp/seats/*.json` | Which tier and serve profile one MCP server process runs at |

Everything in the database is published through a preview/apply pair with a
content hash: the apply is compared against the hash the preview returned, so a
specification cannot change between the two.

## Jira-derived backlog and topology names

An epic apply accepts `epic_backlog_code`. Omit it to allocate from the epic
title or set it explicitly when the business namespace differs from initials —
for example `KOP` for “Kontor Operational MVP”. The value is immutable and
case-insensitively unique within that Kontor project. Jira continues to own full
issue keys such as `ASMA-8001`. Epics created before schema v72 remain readable
without a code; reapply them through the preview/apply pair to assign one before
selecting topology v4.

Schema v73 keeps failed create attempts as immutable incident evidence while
allowing a later explicit `link` materialization for the same epic/task and
stable marker. Only create intents retain marker uniqueness, so recovery cannot
emit a second Jira object; the linked key must still be read back and confirmed
before an item code exists.

Operational topology v4 (`01936f5a-1000-7000-8000-000000000001`, revision `4`)
uses the typed `ITEM_CODE` projection and renders centered-dot names. Enable it
in this order:

1. Preview/apply the epic graph and read back its active epic backlog code.
2. Preview/apply Jira materialization and confirm the epic and task issue
   readbacks. A requested or imported key alone is insufficient.
3. Preview/apply the project topology selection to revision 4, which controls
   what new epics inherit.
4. Preview/apply each existing epic's topology upgrade to revision 4.
5. Preview/apply native-name reconciliation for containers or seats that were
   already materialized under revision 1.

The corresponding `/v1` operations are `epics:preview` / `epics:apply`,
`jira:preview` / `jira:apply`, `topology-selection:preview` /
`topology-selection:apply`, `topology:upgrade-preview` /
`topology:upgrade-apply`, and `native-names:preview` / `native-names:apply`.
Revision 1 is never rewritten. Revision 4 refuses placement before a runtime
mutation when the active epic namespace or one unambiguous confirmed Jira
binding is missing.

## Seat supervision

Copy [`config/examples/paseo-supervision.yml`](../config/examples/paseo-supervision.yml)
to `<state-root>/supervision.yml` to publish the intended policy for validation
and inspection. This does **not** enable runtime watchdog behavior today. If the
file is absent, Kontor invents no timeout or watchdog behavior; if it is present,
Kontor reads and validates it but does not act on it yet.

Normal completion is notification-first: the orchestrator yields after dispatch
and the runtime wakes it on completion, error or permission. The watchdog is an
independent bounded observer for a turn that never completes. It may classify a
suspected hang only when both active-turn age and missing-progress evidence are
stale. Recovery reconciles the same seat first; it never duplicates a seat or
cancels running work.

The YAML contains prompt paths and required skill names. Kontor validates and
exposes those references but does not interpret their names. The intended
consumer will have the selected runtime adapter load their contents, keeping
Paseo, AO, Codex and future adapters on the same policy shape without hard-coded
provider behavior; no adapter is dispatched from this policy today.

> **Status:** the policy is read and validated, and has **no consumer yet** —
> nothing currently acts on a configured watchdog. Absent configuration correctly
> invents no behaviour; present configuration also does nothing until
> `KON-OP-21` wires it. This is recorded rather than implied.

## Seat permission posture

What a seat may do before it has to ask a human is declared, not inherited from
whatever the machine's harness config happens to carry. Operators write one of
three words; Kontor maps each to one internal `SeatAutonomy`.

| Written in `runtimes.json` | Means | Internally |
| --- | --- | --- |
| `autonomous` | Act within what Kontor already authorized, without asking again per tool call | `Bounded` |
| `ask` | Ask a human before each guarded action — the default, and what every seat did before this field existed | `Supervised` |
| `plan` | Read and propose, never act | `Advisory` |

`permission_posture` on a Paseo plane is a **default**, and it is resolved
most-specific-first:

1. the role slot's own `autonomy`, when the frozen team template declared one;
2. the plane's `permission_posture`;
3. `ask`.

A template that already decided keeps deciding, even when the plane default is
the wider one. A realm that declares nothing at either level behaves exactly as
it did before the field existed, which is why a generation-4 document can be read
without migrating it: absence resolves to `ask` and never widens a seat.

### How each provider is told

Posture is translated once, by a single renderer shared between the launch and
the readback that verifies it, so what a seat is spawned under and what Kontor
later checks cannot drift apart.

| Provider | `autonomous` | `ask` | `plan` |
| --- | --- | --- | --- |
| `claude` | `bypassPermissions` | `auto` | `plan` |
| `codex` | `full-access` | `auto-review` | *refused — Codex has no read-only mode* |
| `cursor` | `agent` | `ask` | `plan` |
| `opencode` | `build` + permission block | `build` + permission block | `plan` |

OpenCode is the exception because its mode is not its posture: `build` is
documented by the provider as "executes tools based on configured permissions",
so the posture has to be written where opencode reads permissions. OpenCode
evaluates a call by **last match** over the rules in the order the file gives
them, defaulting an unmatched tool to `ask`; Kontor's block serializes its keys
lexicographically, and `*` — a prefix of every floor pattern — therefore precedes
all of them, which is what lets the specific denials win. Kontor
composes it into `<cwd>/.opencode/opencode.json` at spawn — merged, so an
unrelated key in that file survives, and kept out of the seat's own diff through
the worktree's `info/exclude`. The repository's own committed `opencode.json` is
never edited: git applies no ignore rule to a tracked file, and opencode reads
`.opencode/` at higher precedence anyway.

### The destructive floor

Under every posture that writes a block, these patterns are **denied**, never
asked:

`*submodule update*`, `*submodule deinit*`, `*git rm --cached*`, `*git clean -*`,
`*rm -rf *`

`deny` and `ask` are not interchangeable here. `ask` blocks and waits for a
human — on 2026-08-22 that stalled an eleven-ticket epic for about two and a half
hours, with twelve of fifteen seats wedged on prompts nobody was watching, while
Kontor recorded them as running. `deny` refuses instantly and the seat keeps
working. Autonomy and guardrails stop being in tension once the patterns that
would earn a refusal are refused rather than escalated.

A ticket whose actual job collides with the floor declares a bounded exception on
its task scope:

```json
"task_scopes": {
  "<task-id>": {
    "permission_overrides": ["*git rm --cached*"]
  }
}
```

An override must name a floor pattern **exactly**. Allow-only, and a pattern that
is merely broader — `*git*`, `*rm*`, `*submodule*` — is refused along with
wildcards, near-misses, case variants and unknown patterns, at the type and again
when the fleet is composed. The reason is the evaluation order: OpenCode resolves
a call by *last* match, the block reaches it in lexicographic key order, and a
broader pattern sorts after the deny it overlaps. `"permission_overrides":
["*git*"]` would therefore be evaluated after both git denies and erase them —
one line that never spells `*`. Restricting an exception to an exact floor key
means it can only flip a deny that already exists, in the position it already
occupies, so the set of rules never changes and the order cannot be exploited.

An override reaches the permission block alone — never the mode — so a
task-scoped relaxation can never make a seat verify as something it is not.

> **Operational precondition.** OpenCode *merges* configuration rather than
> replacing it, and the `ask` block deliberately names only `bash` — so on a host
> that still carries the machine-local 2026-08-22 stopgap in
> `~/.config/opencode/opencode.json`, every other tool resolves from that ambient
> config, and an `ask` seat will still edit files without asking. On a clean host
> the unlisted tools fall to OpenCode's own default, which its evaluator gives as
> `ask`, and the posture means what this page says for every tool. Until the
> stopgap is removed from operator machines, `ask` is guaranteed for `bash` only.
> This is a strict improvement on the prior state, where `bash` was ambient too;
> removing the stopgap is out of scope here and tracked with it.

> **Status:** OpenCode and Cursor also expose an `auto_accept` per-agent feature.
> Kontor derives the intended value alongside the mode, but nothing sets it:
> verified against Paseo 0.6.1, neither `paseo agent run` nor `paseo agent update`
> exposes a flag for it, and Kontor drives the CLI rather than the MCP surface
> where it is settable. The permission block is the mechanism that actually
> holds. Recorded rather than implied.

## Seat MCP surface

A Kontor MCP server process holds exactly one credential tier and therefore *is*
one seat; running at two authorities means running two servers. Within that tier
a **serve profile** may narrow which tools the server lists and admits — never
widen it, and never beyond what the credential already allows.

| Seat file | Tier | Serve profile |
| --- | --- | --- |
| `paseo-lead.json` | `admin` | none — the whole vocabulary |
| `worker.json` | `operator` | `worker` — 18 tools: read its work, claim, settle a turn, record a gate verdict, session follow-up, intake, memory search/propose, resolve context |
| `reviewer.json` | `observer` | none — reads only |

Profiles are declared in the registry beside the tier declarations, deliberately
not in the seat file: a free-form tool list in configuration would be a second
authority model that drifts. An unknown profile name refuses to start, and a tool
the profile excludes is refused at call time as well as hidden from the list. See
[`../crates/kontor-mcp/seats/README.md`](../crates/kontor-mcp/seats/README.md).

## Other deployment data

- Profile packs define phases, gates, artifacts, budgets and runtime routing.
  The bundled manifest declares 17 work-profile categories; four ship today
  (`code`, `ux-ui-layout`, `research`, `docs`).
- Team packs define role slots, skills, contexts and handoffs. Role slots carry
  stable ids, so two peers in the same role are explicit rather than duplicate.
- The standard role catalog defines 56 role codes across 9 segments. Seat
  selection is by `role_code`; a free-form role string is not accepted anywhere.
- Account profiles contain non-secret provider-routing metadata and a credential
  reference. No surface — DTO, row, log, export or process argument — has a field
  for a secret value.
- Completion profiles name the integration team, the verdict committee, the
  number of remediation rounds and an optional polling fallback. The seeded
  `operational_default` allows one remediation round.
- Native container and seat naming is a deterministic configurable template.

Changing a prompt, duration, template or specification changes configuration.
Changing a safety invariant requires an architectural decision and code review.
