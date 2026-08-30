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
| `opencode` | *refused* | *refused* | *refused* |
| `cursor` | `agent` | *refused* | *refused* |

Cursor is refused for `ask` and `plan` rather than mapped to its modes of those
names. Its ACP runtime permits shell writes in `plan`, and shell *and* file
writes in `ask` — the same measured finding that keeps cursor out of
consultation. A mode label is not a permission boundary, and a posture Kontor
cannot enforce is refused before launch rather than reported as held. `agent`
means what `autonomous` means, so that one stays.

OpenCode is refused at every posture. It carries its posture in a permission
block rather than in a mode — `build` says nothing about what a seat may do, and
`plan` is behavioural guidance whose canary showed shell writes proceeding — and
**Paseo 0.6.1 offers no way to deliver that block to the spawned process.** A
posture Kontor cannot enforce is refused before launch rather than reported as
held, exactly as for Cursor above.

### Why no OpenCode delivery seat launches

The refusal lands before any transport call, native effect or worktree write.
Three carriers were tried; each fails for its own reason.

**Files and environment variables are outranked.** OpenCode merges configuration
rather than replacing it, and the layers resolve as

```text
global -> OPENCODE_CONFIG -> project -> OPENCODE_CONFIG_DIR
       -> OPENCODE_CONFIG_CONTENT -> active-org remote config
       -> managed config/preferences -> OPENCODE_PERMISSION
```

so nothing Kontor writes is the last word. Merging is per key and per nested key,
and a rule the block does not name — from an auth-backed active-org config or a
system managed profile, both of which sort late — survives and, because
permissions resolve by last match, beats the destructive floor.

**`providerOptions` on the create is inert.** It looks like it sidesteps all of
that: the daemon validates the object against OpenCode's own schema and persists
it on the agent record. But it never reaches a seat. Read from the installed
bundles: Paseo imports `@opencode-ai/sdk/v2/client`, whose `promptAsync`
allow-lists the body keys `messageID, model, agent, noReply, tools, format,
system, variant, parts` and whose `buildClientParams` silently discards the rest;
and OpenCode 1.18.15's `SessionPrompt.prompt` builds its session rules from
`Object.entries(t.tools)` alone. A create the daemon accepts proves the daemon
parsed it, and nothing more.

A seat launched that way would run under whatever OpenCode's own layers resolved
— on a clean host, the evaluator's `ask` default for every unmatched tool. That
is the 2026-08-22 wedging this posture work exists to prevent, so the launch is
refused instead.

**What would lift the refusal.** Paseo attaching the typed permission through
OpenCode's `session.create` or `session.update` — both of which do allow-list it,
and where `SessionHttpApi.update` installs it via `setPermission` — and returning
a correlated applied-policy acknowledgement or effective-permission readback that
a launch can bind on. That is an upstream dependency; see
`docs/evidence/KON-OP-20/2026-08-31-upstream-dependency-applied-permission.md`.

The renderer, the destructive-deny floor and the exact-membership allowance rule
are all still here and still tested. They specify the posture; they are simply
not deliverable yet.

> **Operational note.** The 2026-08-22 machine-global config stopgap that some
> operator hosts carry is, for now, the only thing governing an OpenCode seat's
> permissions. Kontor neither writes nor relies on it.

> **Status:** OpenCode and Cursor also expose an `auto_accept` per-agent feature.
> Kontor derives the intended value alongside the mode, but nothing sets it:
> verified against Paseo 0.6.1, neither `paseo agent run` nor `paseo agent update`
> exposes a flag for it, and Kontor drives the CLI rather than the MCP surface
> where it is settable. Recorded rather than implied.

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
