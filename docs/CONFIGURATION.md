# Configuration

> **Checkout disposition — 2026-09-05:** This document accompanies the older `e6da270` branch baseline and preserves its pending/uncommitted work. Its implementation counts and “no engine” statements describe that baseline, not the current released control plane. For released source `082b63ad`, schema/tool counts, merged OP-21 supervision and OP-22 convergence, use the [revision-stamped inventory](https://github.com/Carasent-ASMA/asma-modules/blob/master/_docs/ai-orchestration/reference/2026-09-05-11-36-reference-kontor-implementation-inventory.md). Release-documentation corrections are isolated from this checkout’s runtime changes.

Kontor separates invariants from deployment behavior. Rust enforces safety
properties such as one non-terminal session per role slot and uncertainty not
being completion. Durations, prompts, skills, profiles, committees, completion,
budgets and runtime routing are versioned data. One pinned Team Definition JSON
revision owns hierarchy, native prefixes/templates, exact seat labels, roles,
slot capabilities and ordering. See [`NATIVE_NAMING.md`](NATIVE_NAMING.md).

That split is the point, not an implementation detail: the workflow being data is
what lets Kontor run research, architecture, UX, QA and operations work without a
core-code branch per work type, and what lets an operator's own conventions become
system behaviour instead of instructions somebody has to remember.

## Where configuration lives

| Location | Holds |
| --- | --- |
| `<state-root>/kontor.db` | Every versioned specification published through `/v1`: Team Definitions, topology specs, role catalogs, work profiles, team templates, advisor profiles, committee templates, completion profiles, Core Team revisions, connector field/workflow specs; also Team Definition defaults, epic pins and migration evidence |
| `<state-root>/runtimes.json` | Runtime family, plane endpoint, per-account provider aliases and the plane's default seat posture. Schema generation `5`; generation `4` is read as a `5` that declares no posture, which resolves to `ask`; generation `3` is refused rather than upgraded, because it can compose the right sessions under misleading names |
| `<state-root>/supervision.yml` | Optional seat supervision policy. Schema v1 is validation/classification only; schema v2 can explicitly enable resident bounded succession (see below) |
| `<state-root>/quota-signals.yml` | Vendor exhaustion wording, applied to a seat's own refusal text (optional; see below) |
| `<state-root>/fleet.yml` | Live model routing — domains, accounts, models, chains and seat bindings, read at every placement (optional; see below) |
| `<state-root>/fleet-activation.json` | Generated: which published fleet policy placement reads, by content hash — and, for an aligned (schema_version 2) activation, which orchestration bundle and Core Team roster. Written only by activation; while it exists, `fleet.yml` is not read (see below) |
| `<state-root>/core-team-history/`, `<state-root>/orchestration-history/` | Generated, immutable: canonical Core Team revisions and orchestration bundle manifests, each named by its content hash (see below) |
| `<state-root>/memory-cognee.json` | Optional bounded experience-retrieval configuration; disabled by default (see below) |
| `<state-root>/credentials.json` | The realm's three tier secrets, `0600` |
| `<state-root>/endpoint.json` | Where the realm listens, when not on the default loopback port |
| `<state-root>/provider-homes/` | One credential home per provider account — `CODEX_HOME` for Codex, `CLAUDE_CONFIG_DIR` for Claude |
| `crates/kontor-mcp/seats/*.json` | Which tier and serve profile one MCP server process runs at |

Everything in the database is published through a preview/apply pair with a
content hash: the apply is compared against the hash the preview returned, so a
specification cannot change between the two.

## Experience memory transport (ASMA-8158 source candidate)

`memory-cognee.json` is optional, strict JSON, at most 8 KiB. Absence keeps
typed, bounded lexical recall enabled and performs no Cognee network work.
Only explicitly `provider_eligible` approved revisions enter the store's minimal
projection preview. The default document policy remains `local_only`.

```json
{
  "enabled": false,
  "endpoint": "http://127.0.0.1:8000",
  "credential_alias": "experience-fixture",
  "dataset_prefix": "kontor",
  "timeout_ms": 1500
}
```

| Key | Default and contract |
| --- | --- |
| `enabled` | `false`; explicit transport opt-in, separate from a document's projection policy |
| `endpoint` | `null`; self-hosted HTTP loopback or HTTPS, without userinfo, query or fragment |
| `credential_alias` | `null`; non-secret alias, never a credential value or a Jira credential |
| `dataset_prefix` | `kontor`; v1 requires this value to preserve the accepted immutable `kontor_<project_uuid>_<memory_cursor>` dataset identity |
| `timeout_ms` | `1500`; 1–1500 ms for the entire semantic operation, reserving at least 500 ms of the 2-second target for authoritative lexical selection |

This source candidate qualifies **synthetic transport only**. Embeddings and
fixture tests can compose `Client::new(config, synthetic_secret)` with
`DaemonConfig::with_memory_cognee`. Ordinary startup validates the document and
refuses `enabled: true` with static `cognee_unavailable` until an authorized
credential/transport composition is supplied. It never reads Keychain, provider
credentials or environment secrets. ASMA-8159 owns live credential resolution,
release qualification and projection activation; configuration here does not
claim those capabilities are deployed.

The transport sends multipart `datasetName` and repeated `data` text files to
`/api/v1/add`, then blocking `/api/v1/cognify`, then `/api/v1/search` with
`search_type: CHUNKS`, a dataset-name filter and `top_k: 64`. CHUNKS text is
parsed only for the embedded canonical identity; backend distance is negated
into descending rank. The returned lesson/cues never supply prompt content.
Every candidate is rehydrated and frozen by the canonical store transaction.
The adapter's `qualify` method produces add/cognify/canary evidence and **does
not activate a dataset**. Live ingestion/chunk identity and release-specific
completion/dataset behavior require ASMA-8159 qualification.

Search/response work is bounded to 64 candidates and 256 KiB before decoding;
redirects, automatic retries and inherited proxies are disabled. Errors expose
static reasons without upstream bodies. An absent/stale/failed/malformed/empty
projection degrades to eligible FTS with no `list_memory` fallback. Recall
selects at most 8 whole experiences within 32768 exact canonical UTF-8 bytes,
including JSON array overhead. The general Context Pack ceiling remains 1 MiB.
Context `memory_selection.selector_version` is now 2, with `ceiling_bytes: 32768`
and `narrowed: true`. `omitted` stays empty because it cannot enumerate an
unbounded corpus; frozen recall metadata supplies exclusion counts instead.

Normal delivery root prompts append the exact frozen canonical array inside
`<experience_memory>` and identify its project, original run, result hash and
block hash. Downstream roles cite that same binding. Replays load historical
canonical bytes with hash verification before retrieving again; approval,
tombstone, task edits and projection changes do not replace the selection.
An explicit purge returns the accepted typed refusal and preserves the binding.

## Jira-derived backlog and topology names

An epic apply accepts `epic_backlog_code`. Omit it to allocate from the epic
title or set it explicitly when the business namespace differs from initials —
for example `KOP` for “Kontor Operational MVP”. The original assignment is immutable and
case-insensitively unique within that Kontor project. Eligible legacy-imported epics have the one-time effective-code correction described in [Native naming](NATIVE_NAMING.md#legacy-import-code-correction); original assignment evidence is preserved. Jira continues to own full
issue keys such as `ASMA-8001`. Epics created before schema v72 remain readable
without a code; reapply them through the preview/apply pair to assign one before
selecting topology v4.

Schema v74 recovers a failed create attempt in place: the original batch, create
intent and marker remain immutable, while an append-only recovery row authorizes
adoption of an exact existing issue. Recovery requires the requested Jira key,
project, issue type, epic parent, summary, description and stable marker to match
the original plan and maps results by ordinal, not connector response position.
An ordinary explicit `link` still validates key/project/type/parent without
claiming ownership of an existing issue's summary, description or workflow
status; it cannot silently replace the original create batch.

Operational topology v4 (`01936f5a-1000-7000-8000-000000000001`, revision `4`)
still validates legal hierarchy and native projection capabilities. Its
centered-dot templates are historical compatibility bytes, not current naming
authority. ASMA Operational Team Definition v1
(`01936f5a-2000-7000-8000-000000000001`, revision `1`) owns the recommended
` • ` rendering in [`NATIVE_NAMING.md`](NATIVE_NAMING.md). Migrate in this
order:

1. Preview/apply the epic graph and read back its active epic backlog code.
2. Preview/apply Jira materialization and confirm the epic and task issue
   readbacks. A requested or imported key alone is insufficient.
3. Validate/publish the Team Definition revision and preview/apply the project
   default selection under compare-and-swap. This affects future epics only.
4. Inventory explicit topics for every legacy ASW/CSW; never derive one from a
   question, title or transcript.
5. Align Kontor lifecycle with runtime-archived history through the supported
   settle, seat-retire, node-retire and node-archive operations. Only retired or
   archived nodes and inactive seats are excluded from migration; their native
   names remain historical. Never retire active work to evade a preview refusal.
6. Reconcile every legacy ticket TSW through `topology:materialize` using its
   stable historical key where available. The selected/pinned definition maps
   each open TeamRun's exact slot to one logical SeatBinding without creating or
   replacing a native session. Replay it again to prove the same binding ids.
7. Preview the existing epic's Team Definition upgrade. Confirm the complete
   identity-bound container-and-seat census before apply. Preview first
   preflights every exact slot of every live TeamRun against the target
   definition and performs no runtime read when a mapping is missing or two
   co-resident slots would render the same name.
8. Apply with one stable idempotency key. A partial result keeps the old pin and
   fences materialization; replay the same key until every exact native object
   reads back and the pin switches. The fence blocks admission, replacement,
   seat release and topology lifecycle transitions before any command write,
   logical retirement or runtime contact. The final persistence check is in the
   same immediate transaction as each seat/node lifecycle write, so lifecycle
   cannot race a frozen migration census.

Logical epic creation may freeze the selected Team Definition before step 2.
This is safe because a pin is not placement authority: every native
materialization path independently requires the active immutable epic code and
the exact confirmed Jira binding for its scope.

The corresponding `/v1` operations are `epics:preview` / `epics:apply`,
`jira:preview` / `jira:apply`, `team-definitions:validate` /
`team-definitions:publish`, `team-definition-selection:preview` /
`team-definition-selection:apply`, and `team-definition:upgrade-preview` /
`team-definition:upgrade-apply`. Historical definitions and topology revisions
are never rewritten. Placement and migration refuse before runtime mutation
when the active epic namespace, confirmed Jira binding, topic, definition pin
or exact identity readback is missing or ambiguous.

The recommended TSW `team_slots` are exactly `scope→SA`, `implement→SWE`,
`verify→QA`, and `audit→AUD`. These mappings are separate from fixed local
`slots`. A definition catalog may contain alternative-template slot ids that map
to the same role code, but a frozen TeamRun containing two slots that render the
same name is refused before runtime contact. Unknown slots are never mapped from
their spelling or logical role. In particular, Research Spike remains
unregistered until a future `SLOT_DISPLAY_NAME` revision can name its two `BA`
seats distinctly.

All seats, including ECP/ASW/CSW local slots and TSW delivery slots, resolve
through the same exact `(container kind, RoleSlotId)` lookup. The configured
role code or display label is authoritative; persisted roles and caller values
are never fallback names. Migration record and confirmation each compare the
complete live census bidirectionally by subject and immutable native identity,
so neither an omitted live object nor a stale extra target can move the pin.
TeamRun slot preflight is likewise limited to active topology: a nonterminal
run whose exact seats and node were already retired remains history and cannot
block the current pin upgrade.

Schema v77 introduced Team Definitions and migration state; v78-v80 complete
per-seat advice, receipt recovery and exact command-intent recovery. During a
v79→v80 upgrade, only a migration with a bound
`upgrade_team_definition` command receipt can recover its exact intent hash.
Any unreceipted recorded, applying or confirmed legacy migration is retained as
an explicit `legacy_unrecoverable` fence and returns a typed conflict; Kontor
never substitutes the migration fingerprint or target set for the missing
command. A deployed naming migration is therefore healthy only at schema v80
or later and only when no such recovery fence remains.

Redacted export generation 4 introduced seven Team Definition record arrays.
When reading supported generations 2 or 3, Kontor supplies those absent arrays
only as empty in-memory defaults. It removes them again for legacy canonical
hashing, continuity comparison and serialization; a genuine generation-3
export therefore verifies byte-for-byte without being rewritten into a false
generation-4 shape.

## Jira reconciliation

Kontor is authoritative for desired orchestration state; Jira remains the
external workflow system. The daemon automatically converges every task and
epic that has an exact confirmed Jira binding. No operator-triggered `jira
sync` command is required for ordinary lifecycle, gate, completion or backlog
changes.

The resident controller waits for the startup reconciliation barrier, performs
an immediate pass, reacts to committed control-plane append signals, and runs a
30-second backstop for missed notifications and restart recovery. Durable
Kontor state is the queue; there is no second in-memory desired-state ledger.
An unchanged conflict or a failed external effect waits for the bounded
backstop instead of waking an immediate retry loop.

Selection is exact and fail-closed:

- task reconciliation selects by `connector.jira`, external project, issue
  type, frozen work-profile id and frozen work-profile revision;
- epic reconciliation selects the generic epic policy and reads only epic
  completion plus child-task evidence;
- the selected bundled workflow must have an identical installed immutable
  revision in the project before any Jira write;
- task identity comes from the canonical task-to-Jira ledger, while epic
  identity comes from its confirmed epic binding; display item codes and
  native names are never reverse-parsed into Jira keys.

Install each required workflow revision through
`connectors/{connector}/workflow-specs:install` (or the matching Kontor MCP
tool), using a fresh project revision and a stable idempotency key. A project
with high-stakes tasks and a Jira epic normally needs both the high-stakes task
revision and the generic epic revision installed. Read the workflow catalog
back and require `installed: true` for each exact selected revision.

Every external transition is derived from a fresh issue observation and the
currently offered destination transitions. Epic writes first persist immutable
transition authority, then apply the Jira effect, refetch the issue, and confirm
the intent only from that readback. Ambiguous or contradictory evidence is
never guessed. Conflicts are append-only, de-duplicated by subject and kind,
and stay open until an authorized explicit resolution records its receipt.

A milestone may declare an ordered `route` of exact `from` and `to` status
selectors when the external workflow cannot reach its final target in one
transition. Routes are configuration, not graph search: from the freshly
observed status Kontor selects only the one declared next destination, requires
exactly one currently offered transition to it, confirms that intermediate
destination, and then reconciles the next hop from a new observation. Every
declared status must exist in the same immutable workflow revision, every
source is unique, and each chain must terminate at that milestone's final
target without a self-edge or cycle. An undeclared, unavailable or ambiguous
step fails closed as a typed conflict; Kontor never chooses a plausible Jira
path from names or whichever transitions happen to be live.

The bundled ASMA generic epic workflow revision 2 declares the observed Jira
route for an active epic explicitly: `New (10227)` → `DRAFT (10237)` →
`TO BE GROOMED (10236)` → `Groomed (10233)` →
`READY FOR DEVELOPMENT (10213)` → `In Development (10214)`. This route was
verified from current ASMA Epic transitions and Epic changelog evidence; it is
not inferred from status wording. Installed revision 1 remains immutable for
historical readback, but new selection and installation use revision 2.

Completion continually re-evaluates child work after it leaves the ticket gate.
A task added or reopened during integration, Committee review, closeout or a
finished era returns the epic to its ticket gate under a new attributed era;
prior integration, verdict, remediation and closeout evidence remains immutable
history. Jira therefore cannot stay successfully closed over newly unfinished
child work.

## Seat supervision

Copy [`config/examples/paseo-supervision.yml`](../config/examples/paseo-supervision.yml)
to `<state-root>/supervision.yml` only when this Realm should opt into the
resident succession engine. Enablement is deliberately explicit:

- with no file, no supervisor starts;
- schema v1 remains readable for legacy classification and starts no automatic
  succession;
- schema v2 requires `recovery.max_concurrent_successions`, rejects zero and
  values above the process safety bound, and starts the supervisor only when
  `watchdog.enabled` is `true`;
- a disabled schema-v2 watchdog starts no supervisor even when the concurrency
  field is present.

This prevents a daemon upgrade from silently assigning a cadence or concurrency
ceiling to an existing Realm. The shipped example selects schema v2, a
300-second cadence and five concurrent succession sagas; those are deployment
choices, not kernel defaults.

The declared normal mode is notification-first: the orchestrator yields after
dispatch and the configured orchestration surface is expected to wake it on
completion, error or permission. The watchdog is an independent bounded observer
for a turn that never completes. It may classify a suspected hang only when both
active-turn age and missing-progress evidence are stale. Recovery reconciles the
same seat first; it never duplicates a seat or cancels running work.

The `completion` block remains the orchestration policy contract; KON-OP-21 does
not add a notification transport. Its resident supervisor is the bounded
durable-recovery backstop described below, not a replacement event bus.

The resident loop waits for the startup reconciliation barrier, first resumes
due durable attempts, then rebuilds its inventory on the configured cadence and
on committed append signals when `runtime_error` is configured as a wake
condition. It evaluates only nonterminal active TeamRuns and only a blocked seat
whose latest reachable runtime observation,
binding generation, account, provider quota row and immutable
runtime-observation provenance match exactly. Durable attempts are both queue
and slot lock; restart and replay resume them rather than creating another
successor. Hang classification remains read-only and is not silently converted
into quota succession.

The YAML also contains prompt paths and required skill names. Kontor validates
and exposes those references but does not reinterpret or execute them in the
resident loop. They remain orchestration-surface metadata; the selected runtime
adapter remains responsible for native inspection and placement.

> **Release status — 2026-09-05:** KON-OP-21 is implemented and merged through PR #170 (`080e2db3`, 2026-09-05), included in the inspected release `082b63ad`. Local contract coverage is recorded; independent qualification, coherent fleet deployment and realm enablement require their own receipts and are not certified by this documentation refresh.

See the [canonical implementation inventory](https://github.com/Carasent-ASMA/asma-modules/blob/master/_docs/ai-orchestration/reference/2026-09-05-11-36-reference-kontor-implementation-inventory.md) for source and deployment distinctions.

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
| `opencode` | `build` + applied block | `build` + applied block | `plan` + applied block |
| `cursor` | `agent` | *refused* | *refused* |

Cursor is refused for `ask` and `plan` rather than mapped to its modes of those
names. Its ACP runtime permits shell writes in `plan`, and shell *and* file
writes in `ask` — the same measured finding that keeps cursor out of
consultation. A mode label is not a permission boundary, and a posture Kontor
cannot enforce is refused before launch rather than reported as held. `agent`
means what `autonomous` means, so that one stays.

OpenCode carries its posture in a permission block rather than in a mode —
`build` says nothing about what a seat may do, and `plan` is behavioural guidance
whose canary showed shell writes proceeding. It launches only when the daemon
both *can* apply that block and *says it did* for the exact agent.

### How an OpenCode seat is launched

**One `create_agent_request`, carrying everything:**

- `config.providerOptions.permission` — the rendered block;
- `config.mcpServers` — the typed seat MCP surface;
- `initialPrompt` — the first turn, a **top-level sibling of `config`**;
- `clientMessageId` — likewise top-level, derived from the launch rather than
  generated, so a retry asks about the same turn;
- `labels`, also top-level, carrying a launch-intent digest over the whole
  outgoing message *and* the prompt.

The envelope is not a guess. `CreateAgentRequestMessageSchema`
(`packages/protocol/src/messages.ts`) declares `initialPrompt`, `clientMessageId`
and `labels` as siblings of `config`; `handleCreateAgentRequest`
(`packages/server/src/server/session.ts`) destructures both from the message and
passes them to `createAgentCommand`; and the answer is a `status` frame carrying
`agent_created`, the `requestId` and the agent payload built from the live
snapshot. Read from `paseo-op20-v0.6.1-backport`, deploy pin `a07ed03e0`.

There is no second stage. An earlier design created the seat empty and sent the
first turn separately so acceptance could stand as proof; with the daemon now
reporting application on the agent itself, that turn proves nothing the snapshot
does not already say, and two effects to reconcile instead of one is pure hazard.

**Two gates, and the difference between them matters.** Before any native call,
the daemon must advertise `providerOptionsApplied`. After the create, the
returned agent must report `providerOptionsApplied: true`. The feature says the
daemon *can*; the per-agent flag says it *did*. A launch binds on the second.
Missing and `false` are both refusals.

The gate is deliberately **not** a version. Kontor shipped a version-gated path
once whose permission the daemon validated, persisted, and dropped before it
reached the provider: the v2 SDK's `promptAsync` allow-lists its body keys, and
OpenCode's own prompt route reads only `t.tools`. The version was right and the
policy never applied.

**The seat is judged from the create's own snapshot** — placement, route, and the
acknowledgement — with no follow-up fetch, because a later read answers a
question about a later moment.

### When the answer is ambiguous

A create whose answer is lost may still have landed, so it is never sent again.
Reconciliation is an exact-label paginated census on the launch intent:

| The census finds | What happens |
| --- | --- |
| one match, unbound, **and its first turn proved** | adopted — it is this launch's own effect |
| one match, unbound, first turn not provable | refused; it was created and never told anything |
| one match, already bound to a run | refused; one session may not have two owners |
| none, on a complete enumeration | `DeliveryConfirmationUnknown` — **the seat claim is kept** |
| several, or an enumeration that did not finish | quarantined: no adoption, no create, no release |

Labels prove a create happened; they do not prove the turn did. The create sends
the prompt *after* the agent exists, so an agent can carry this launch's exact
intent and never have been prompted — adopting it would seat a run on a session
that sits idle forever while the launch reports success. Recovery therefore
requires the launch's `clientMessageId` on the agent's **canonical** timeline,
scanned backward from the tail with bounded pages under one fixed epoch. An
absent id, an unfinished scan, a renumbering mid-scan, or a daemon-reported gap
all refuse.

Keeping the claim is the point, and **nothing releases it on an ambiguous
outcome** — not `agent_create_failed`, not the typed `agent_create_unresolved`,
not an unrecognised status. Releasing would let the next attempt take the slot
and create a *second* agent for a run that may already have one.

The deploy carrier (`a07ed03e0` on exact v0.6.1; `661536df9` on main) does
distinguish those two words: it records the native id before sending the initial
prompt, attempts an exact-agent archive if the create then fails, and reports
`agent_create_failed` only once that compensation is **confirmed** —
`agent_create_unresolved`, naming the agent, when it is not. The revision before
it (`a878145`) could not: the id was captured after the prompt, so a throwing
prompt left it null and a create failure was reported while the agent ran.

Kontor does not branch on the difference, and does not adopt the agent the
carrier names. Branching would make correctness depend on which build answered,
and a daemon can be rolled back under a running plane. One path — census, then
first-turn proof — serves both.

A created seat that fails any check is archived over the same socket and read
back terminal. The archive *acknowledgement* is not the cleanup: it can be lost
after the daemon has already acted, so the readback runs whether or not the send
was acknowledged, and only a fresh reading of that exact agent as terminal
counts. Live, unfetchable, or an answer about a different agent all refuse
recoverably and keep the claim. A durable bind failure
returns confirmation-unknown too: the intent label is on the agent, so
reconciliation adopts that very seat instead of stranding or duplicating it.

### Why the policy is not a file or an environment variable

OpenCode merges configuration rather than replacing it, and the layers resolve as

```text
global -> OPENCODE_CONFIG -> project -> OPENCODE_CONFIG_DIR
       -> OPENCODE_CONFIG_CONTENT -> active-org remote config
       -> managed config/preferences -> OPENCODE_PERMISSION
```

so nothing Kontor writes is the last word. Merging is per key and per nested key,
and a rule the block does not name — from an auth-backed active-org config or a
system managed profile, both of which sort late — survives and, because
permissions resolve by last match, beats the destructive floor. The create
sidesteps all of it, and the per-agent acknowledgement is what makes that
claimable rather than assumed.

> **Operational note.** A machine-global config — such as the 2026-08-22 stopgap
> some operator hosts still carry — cannot reach a Kontor-launched OpenCode seat:
> the policy is applied to the session by the daemon, not resolved from files. It
> still governs any OpenCode process started outside Kontor.

> **Status:** OpenCode and Cursor also expose an `auto_accept` per-agent feature.
> Kontor derives the intended value alongside the mode, but nothing sets it:
> verified against Paseo 0.6.1, neither `paseo agent run` nor `paseo agent update`
> exposes a flag for it, and Kontor drives the CLI rather than the MCP surface
> where it is settable. Recorded rather than implied.

## Fleet configuration (`fleet.yml`)

Copy [`config/examples/fleet.yml`](../config/examples/fleet.yml) to
`<state-root>/fleet.yml` to let one live file, rather than published template
revisions, own model routing. Kontor re-reads it on **every seat placement** —
seat fill, seat replacement, quota takeover, succession refresh, Committee
invoke, Committee seat recovery and Advisor invoke — so one edit changes the
next placement with no rebuild, restart or template republish.

The file lives in the state root, outside every git checkout, so a branch switch
can never change routing.

### File rules

- `<state-root>/fleet.yml` must be a regular file, not a symlink, owned by the
  same user as the state root, and not writable by group or others (`chmod 600`).
- It is at most 256 KiB and must be UTF-8; unknown fields are rejected at every
  level, and the shipped example is a parseable starting point.
- It names accounts by alias only. It never holds credentials, tokens or
  environment values, and Kontor never logs its contents — only the hash and the
  name of the failing rule.

### Schema

```yaml
schema_version: 1

domains:
  claude:     { provider: claude,   accounts: [claude-personal, claude-work] }
  codex:      { provider: codex,    accounts: [codex-work] }
  cursor:     { provider: cursor,   accounts: [cursor] }
  openrouter: { provider: opencode, accounts: [opencode], model_prefix: "openrouter/" }

unavailable:
  domains: [openrouter]   # a provider that does not work at all
  accounts: [claude-work] # one switched-off login

models:
  opus-5: { domain: claude, id: claude-opus-5, vendor: anthropic, efforts: [xhigh], vision: true, calibrated: true }
  sol:    { domain: codex,  id: gpt-5.6-sol,   vendor: openai,    efforts: [xhigh], vision: true, calibrated: true }

chains:
  claude-first:
    - [opus-5@xhigh]
    - [sol@xhigh]

bindings:
  team/<team_template_id>/implement: claude-first
  committee/<committee_template_id>/reviewer-a: claude-first
  advisor/<advisor_profile_id>: claude-first

rules:
  calibration_required: [team/<team_template_id>/verify]
  vision_required: [team/<team_template_id>/browser-verify]
  independent_of:
    team/<team_template_id>/verify: team/<team_template_id>/implement
```

A **domain** is one failure domain: the accounts that go down together, for
example one vendor plan. A **chain** is an ordered list of **steps**, each step
written as one domain, and each step is walked in **sub-steps**: Kontor tries
every listed model on every account of that domain before it descends to the
next step. `[opus-5@xhigh]` followed by `[sol@xhigh]` therefore means: Opus on
every Claude login, then Sol on every Codex login. Chain length has no fixed
limit; the sanity ceiling is 16 steps and 64 flattened routes.

A **binding** maps `team/<id>/<slot>`, `committee/<id>/<slot>` or `advisor/<id>`
to one chain, and applies to every version of that template or profile. A
binding wins over the frozen template chain; the template chain is used only for
seats the file does not bind. A bound chain that flattens to no admissible route
fails closed — it never falls back to the route the operator replaced.
`core/<role_code>` keys are rejected in v1.

### `unavailable`

`unavailable.domains` switches off a whole domain and `unavailable.accounts` a
single alias; matching routes are dropped while a chain is flattened, so the
next placement skips them. Use it for a provider that does not work at all.
Quota exhaustion needs no entry: Kontor's quota evidence already walks past an
exhausted account. The `runtimes.json` `unavailable_providers` list still
applies on top.

### The three rules

- `rules.calibration_required` — the listed verdict seats (verify, audit, QA,
  Committee) skip every model not marked `calibrated: true`. Empty the list to
  waive calibration.
- `rules.vision_required` — the listed seats skip models with `vision: false`.
- `rules.independent_of` — a seat's routes must avoid the **vendor** the named
  seat last ran on in the same team run, for example
  `team/<id>/verify: team/<id>/implement`. If the named seat has no recorded
  route, placement proceeds and logs `fleet.independence_unknown`.

### Readback and receipts

After every load attempt Kontor rewrites `<state-root>/fleet-status.json`
(`active_hash`, `loaded_at`, `last_error`), so an edit is confirmed or rejected
at once. An invalid edit keeps the last valid snapshot, reports the failing rule
there, and is never applied in part. Every accepted version is stored once under
`<state-root>/fleet-history/<hash>.yml`, and each admitted fleet placement
appends one JSON line, including its step and sub-step, to
`<state-root>/fleet-decisions/<team_run_id>.jsonl`. Rollback is one copy from
`fleet-history/`.

### Turning it off, and removing a model safely

Deleting `<state-root>/fleet.yml` is the supported way to turn the feature off:
every placement then behaves exactly as if the file had never existed.

To remove a model from a live file, **remove it from every `chains` entry
first** — the validator rejects a file that deletes a model while a chain still
names it. Delete it from `models` only once no live seat is frozen on it:
explicit-route commands (bridge moves, `replace_seat` with a named route, and
consultation seat recovery naming the frozen route) re-check that exact route
against the catalog when they run. Automatic quota takeover walks the declared
chain and is not affected.

### Activated fleet policy (ASMA-8280)

The policy both orchestration modes share is authored in the project checkout
and handed to Kontor through four admin operations, which the CLI and MCP
registry serve from the same route table:

| Operation | Effect |
| --- | --- |
| `kontor_fleet_policy_get` | Which source decides routing (`activation` or `fleet_yml`), the activation record, the hash placement reads now, and why an activation cannot be served. |
| `kontor_fleet_policy_preview` | Validates one document (schema_version 1 or 2) and returns its content hash. Writes nothing. |
| `kontor_fleet_policy_publish` | Writes the previewed bytes, unchanged, to `fleet-history/<hash>.yml`. **Selects nothing.** |
| `kontor_fleet_policy_activate` | Atomically replaces the owner-only `fleet-activation.json` with the named published hash, fenced on `expected_active_policy_hash`. |

Once a record exists, placement reads exactly the artifact it names, and the
record and artifact are both re-verified on every read: file rules, content
hash and schema. Editing `fleet.yml`, a checkout, or publishing without
activating has no effect. A record or artifact that fails verification refuses
every placement that resolves a fleet binding, naming the failed check; it
never falls back to `fleet.yml` or to template routing. Deleting the record
returns the Realm to the unmigrated `fleet.yml` behaviour above. Rollback is
activating a recorded hash.

A published policy names accounts, models and seats; it never locates or
authenticates them. Preview and publish refuse any value that carries
credential material, a filesystem path (such as a provider home) or an email
address (V-33); the unmigrated `fleet.yml` reader keeps its v1 checks.

Every reader takes the same activation decision from `kontor-fleet`: only the
bytes at the record's content address, validating under the record's schema,
are the policy. A reader outside the daemon that follows the record's two files
therefore resolves exactly what placement resolves, and its choice for a seat —
the first route in chain order that the stated eligibility admits, with the
reason every other route was passed over — is the same record for the same
policy bytes, key and eligibility.

Schema version 2 keeps every v1 section and rule and adds one binding family,
`leadership/<core-team-revision-hash>/<role-slot-id>`, for epic leadership
seats. The hash is the canonical content hash of the epic's pinned Core Team
revision, so a roster change is a new key. `leadership/lsa`,
`leadership/<slot>` and `core/<role_code>` are rejected. For a bound LSA or TPM
seat, Core Team materialization, route correction and launch-intent
supersession refuse any caller route the bound chain does not offer, and each
admitted leadership effect appends its policy hash, binding, chain position and
occupancy generation to `fleet-decisions/leadership/<seat_binding_id>.jsonl`.
A leadership seat no policy binds keeps its caller-supplied route.

A materialization route may name `eligibility`
(`{unavailable_accounts, excluded_vendors}`) instead of `model_route`. The
activated policy then chooses: the first route in chain order the stated
eligibility admits, the same choice a direct-mode reader makes, and the
decision records that eligibility. A named `model_route` is admitted or refused,
never replaced. A route naming both or neither is invalid; a seat no policy
binds, or whose bound routes are all ineligible, is refused before any seat
exists.

Governed delivery and consultation seats a fleet policy binds are chosen the
same way. The exact quota observation is first stated as an explicit
eligibility — an account alias with no account whose headroom admits a new seat
now is unavailable, and a seat under `rules.independent_of` excludes the vendor
its partner ran on — and the shared resolver (or, for a Committee, the shared
allocator) chooses under it. The scheduler's rules still decide whether that
choice launches now, waits for a near reset on a route it would rather have, or
escalates. The eligibility is recorded with the decision: on each
`fleet-decisions/<team_run_id>.jsonl` line, on a fleet-routed Committee or
Advisor admission, and on a fleet-routed Committee seat recovery profile. A
seat with no fleet binding, and an explicitly named caller route, walk the
unchanged headroom chain and record no eligibility.

#### Aligned activation: the orchestration bundle

A schema_version 2 activation record names one orchestration bundle as well as
its policy. The bundle is authored in `config/orchestration/`:
`orchestration.yml` (`schema_version: 1`, `fleet: fleet.yml`,
`core_team: teams/core-team.yml`) selects the policy and the explicit Core Team
source. `teams/core-team.yml` pins one role catalog by `catalog_id`, `version`
and `content_hash`, and declares every seat in order — the mandatory LSA and
TPM included — with its `role_slot_id`, `role_code`, `presence` and
`ad_hoc_allowed`. The publisher resolves it through the Core Team resolver, and
the canonical revision bytes it produces define the `core_team_revision_hash`
every `leadership/<hash>/<slot>` binding names.

Five admin operations, served by the route table, the MCP registry and the CLI
alike, carry a bundle from proposal to activation:

| Operation | Effect |
| --- | --- |
| `kontor_fleet_bundle_propose` | An initial `orchestration.yml` and `teams/core-team.yml` holding only the mandatory roles, with the exact catalog revision (id, version, content hash) they pin. Writes and activates nothing; nothing reads the proposal until it is published and activated. |
| `kontor_fleet_bundle_preview` | Resolves the exact `orchestration`, `fleet` and `core_team` bytes against the realm catalog revision the Core Team source pins, and returns the manifest publication would write and the preview hash, which binds those bytes and that catalog revision. Writes nothing. |
| `kontor_fleet_bundle_publish` | Resolves the same bytes again, requires that preview hash, and writes and verifies the immutable policy, roster and manifest. **Selects nothing.** |
| `kontor_fleet_bundle_activate` | Names `source_bundle_hash` and `expected_active` — the standing record's `policy_hash`, plus its `source_bundle_hash` when it is a v2 record, omitted entirely only when no record stands — verifies every artifact, and replaces the one pointer atomically. |
| `kontor_fleet_bundle_get` | The record as a bundle: `activation_schema_version` 1 or 2, the record, and the manifest a v2 record names. |

Publish and activate keys are bound realm-wide, each to its complete logical
request. The single-policy operations above still work unchanged and write a
v1 record over the same pointer.

Publication writes three immutable artifacts: the policy under
`fleet-history/`, the canonical roster under
`core-team-history/<core-team-revision-hash>.json`, and a canonical manifest
under `orchestration-history/<source-bundle-hash>.json` recording the resolver,
the hash of each source file, the policy hash and schema, the role-catalog pin
and the roster hash. Activation verifies every artifact first and then replaces
the one pointer:

```json
{
  "schema_version": 2,
  "source_bundle_hash": "<manifest hash>",
  "policy_hash": "<policy hash>",
  "policy_schema_version": 2,
  "core_team_revision_hash": "<roster hash>",
  "activated_at": "<timestamp>"
}
```

Readers verify the pointer, then the manifest (which must agree with it), then
exactly the policy and roster it names, and that the roster was resolved against
the catalog the manifest pins. Under an aligned activation a governed leadership
launch consumes only the selected roster: an epic whose pinned revision is not
those exact bytes keeps its pin, is never retargeted, and its leadership launch
is refused. A schema_version 1 record keeps its exact shape and behaviour and
selects no roster.

A project's Core Team can be published from a published bundle through the
existing `kontor_core_team_preview` and `kontor_core_team_apply`: name
`source_bundle_hash` instead of `seats` (exactly one of the two). Both steps
re-verify the bundle's immutable artifacts, derive the seats from its roster
through the same resolver, and require the result to be the bundle's roster
byte for byte — the bundle must be authored at the project's next Core Team
version against the catalog the realm holds, or the preview refuses. The apply
names the bundle in its intent, receipt and outcome. Nothing else changes: no
epic pin or running seat is retargeted, and an explicit-seat request is
unchanged.

#### Direct-mode resolution without a daemon

`kontor --state-root <root> --tier operator fleet-policy-resolve --binding-key <key>`
(optionally `--unavailable-accounts '[...]'` and `--excluded-vendors '[...]'`) is
the registry's one local operation. It is operator work, which admin inherits;
an observer is refused. It runs in the CLI before any connection — no daemon,
credential file, base URL or network — through the same verified reader the
daemon uses, and prints `{tool, status: 200, body: FleetSelection}`. Nothing
eligible prints the selection under `status: 409`, `placement_blocked`.
Anything missing, unsafe, mismatched or malformed is a local refusal naming its
rule; there is no `fleet.yml` and no last-valid fallback, and a schema_version 1
activation resolves no leadership key. MCP neither lists nor dispatches it: it
has no `/v1` route.

The same operation allocates a whole Committee jointly when it is given
`--allocation` instead of `--binding-key` — exactly one of the two:

```json
{
  "diversity": "distinct_vendor_per_reviewer",
  "slots": [
    {"slot_id": "reviewer-a", "role": "reviewer", "binding_key": "committee/<template>/reviewer-a"},
    {"slot_id": "reviewer-b", "role": "reviewer", "binding_key": "committee/<template>/reviewer-b",
     "unavailable_accounts": ["codex-work"]},
    {"slot_id": "judge", "role": "judge", "binding_key": "committee/<template>/judge"}
  ]
}
```

Each slot states its own eligibility; the top-level `--unavailable-accounts`
and `--excluded-vendors` belong to single mode. Slot ids are unique. The
activation is loaded and verified once, every slot is resolved against that
snapshot, and the allocation is the shared allocator's: slots in the order
given, then each chain in policy order; no two reviewers share a policy vendor,
and a reviewer route whose vendor is `unknown` is not eligible; the judge is
held only to its own eligibility. The answer is one complete ordered allocation
under one shared provenance (policy, and for an aligned activation bundle and
roster) with each slot's binding, chain, eligibility, every candidate
considered and why it was passed over, and the chosen step, sub-step and vendor
— or, under `status: 409` `placement_blocked`, the same receipt with no slot
selected and the reason. There is no partial allocation. Governed Committee
admission calls the same allocator.

The same operation places one `planning_pair@1` pair when it is given
`--planning-pair` instead. Name exactly one of `--binding-key`, `--allocation`
and `--planning-pair`:

```json
{
  "members": [
    {"slot": "seat-a", "binding_key": "advisor/<profile-a>"},
    {"slot": "seat-b", "binding_key": "advisor/<profile-b>",
     "unavailable_accounts": ["codex-work"]}
  ]
}
```

The members are exactly `seat-a` then `seat-b` (PP-01). Each names an existing
binding and states its own eligibility. The top-level `--unavailable-accounts`
and `--excluded-vendors` belong to single mode (PP-04). Combining
`--planning-pair` with another mode is refused (PP-03); `--binding-key` with
`--allocation`, or no mode at all, is J-01 as before. The pair has no role,
Judge or diversity to set. It is one joint allocation of both members through
the same allocator under `distinct_vendor_per_reviewer`: the members never
share a policy vendor, and a route whose vendor is `unknown` is not placed. The
answer is `{protocol: "planning_pair@1", selection, placement_hash, members}`:
`selection` is the joint allocation receipt, `placement_hash` its canonical
hash, and `members` the two frozen members. When no such placement exists, the
answer is the same document without `members` under `status: 409`
`placement_blocked`. It is placement evidence only: it carries no verdict, and
it never satisfies a review gate.

## Provider quota signals

Copy [`config/examples/quota-signals.yml`](../config/examples/quota-signals.yml)
to `<state-root>/quota-signals.yml` to tell Kontor how each vendor words an
exhaustion refusal. The sentences are data on purpose: a vendor rewords its
message far more often than Kontor ships, and encoding them as Rust constants
would make tracking a copy change a rebuild.

Install and read it back:

```sh
cp config/examples/quota-signals.yml "$KONTOR_STATE_ROOT/quota-signals.yml"
$EDITOR "$KONTOR_STATE_ROOT/quota-signals.yml"
# Readback: the daemon refuses to start on a present-but-invalid document, so a
# clean start is the readback. Confirm what it now classifies with:
kontor --state-root "$KONTOR_STATE_ROOT" provider-quota-states-list
```

Each signal carries the provider as the catalog spells it, whether the vendor
charges a plan allowance or a prepaid credit balance, the markers that must all
appear before text is read as a refusal, and — for a plan allowance that states
one — the text preceding the reset instant and the IANA zone a bare wall clock
is printed in. A vendor that prints local time without naming a zone cannot be
read correctly without that field, and guessing wrong shifts the reset by hours.

**Every signal carries an identity, and the alias is not it.** Each entry
declares a stable logical `id`, unique within the document, and a positive
`version` that increments whenever its wording or parsing changes. Two logins of
one vendor carry the same sentence under the same family, so a record naming
only `claude-work` could not say which fingerprint authorized a retirement —
which is why the shipped Claude entries have distinct ids despite identical
wording. A signal's complete definition (id, version, provider, basis, ordered
markers, reset prefix, zone) is digested, and durable provenance cites that
digest: changing any of it under an unchanged id and version produces a
different digest, which immutable history is entitled to refuse.

**`provider` is an exact catalog alias, never a vendor family.** A deployment
addresses one login per alias — `codex-work` and `codex-personal` are two
accounts of the same vendor — and each account's routing document declares
exactly which aliases it may select. A quota state is keyed by
`(account, provider)`, so a signal naming the bare family `codex` matches no
account that routes `codex-work`, and classification for that account is
silently inert. **Name one entry per alias**, repeating the vendor's wording as
many times as the deployment has logins.

**Order is significant, and eligibility is applied first.** The daemon filters
this sequence to the aliases the seat's own account may select, and only then
reads the text; classification returns the first *eligible* signal whose markers
all appear. So repeating identical wording across two aliases is safe —
`codex-work`'s entry can never stand in front of `codex-personal`'s for a seat
running on the personal login. Order still decides between two entries that are
both eligible for one account, which is why the shipped example lists the Claude
aliases before the Codex ones: the whole of the Codex marker set is the words
"usage limit", which a Claude refusal also contains.

**A vendor that restates its zone is checked against the declared one.** Some
messages print `… resets 10:40pm (Europe/Chisinau)`. That annotation is never
read as part of the clock, and it is **compared** rather than skipped: *every*
parenthesized token in the message is checked, and any that names a zone the
tzdb knows must **agree** with `reset_zone`. A disagreement anywhere — including
one hidden behind an earlier unrecognised annotation such as `(EEST)` — yields
no instant at all — the account still blocks, as `Unknown`, which is the
visible prompt to fix the signal. Ignoring it would let a message saying
`(Europe/Oslo)` be converted as Chisinau and land an hour wrong with nothing to
show for it. An abbreviation like `(EEST)` is not an IANA name, cannot be
compared, and is left alone.

**A stated zone is the provenance of a captured message, not the host's
clock.** `reset_zone` qualifies the wall clock *that vendor's message printed*,
recorded alongside the wording it belongs to. It is never inferred from the
daemon's own timezone: a host that later moves to another zone is a fact about
now, and letting it reinterpret a historical fingerprint would silently move
every reset derived from it. The shipped Codex entry states `Europe/Oslo`
because that is where the 2026-08-21/23 incident message was captured, and the
Claude entries state `Europe/Chisinau` because that is what their own
2026-08-30 message printed — neither because any particular machine runs
there.

**Only an exact, distinctive system-refusal fingerprint may activate a signal.**
A bare phrase like `usage limit` is not sufficient: an ordinary assistant
message *discussing* limit handling contains it, and this configuration has the
authority to archive a live seat. Require the vendor's framing, its settings
URL and its retry wording together. A vendor whose refusal has not been captured
stays commented out rather than shipped on unverified copy — a false negative
falls back to the poll and the operator, while a false positive retires work
that was running.

**Absent, unreadable and invalid are three different outcomes.** Only the first
is inert:

| The document is… | Kontor… |
| --- | --- |
| absent | leaves refusal-message classification inert; pre-flight provider polling is unchanged, and a schema-v2 supervisor can resume existing durable attempts but cannot derive a new quota decision from unconfigured wording |
| present but unreadable | refuses to start, with a typed `Read` naming the path |
| present but unparsable or schema-invalid | refuses to start, with a typed `Document` or `Invalid` naming the stable rule |

A broken document is never quietly degraded to "inert". It states an intent the
realm cannot honour, and starting anyway would leave an operator believing
reactive classification is armed when it is not.

> **Status:** the `claude` entry in the shipped example is **provisional** — no
> live Claude refusal has been captured into this repository, so its markers are
> stated from observed phrasing rather than verified against a recorded message,
> and it declares no `reset_prefix`. A Claude refusal therefore records a
> blocking state with **no** stated reset instant until a real message is
> captured and the document corrected. The Codex entry is verified against the
> message recorded on 2026-08-21.

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

The dynamically composed `leadership` profile for persistent LSA/TPM seats is
limited to completion read/remediation and exact Committee-seat permission
inspection/response. A response uses a canonical UUIDv7 `Idempotency-Key` and is
persisted in schema v75 before the runtime effect. Confirmed replay is inert;
confirmation-unknown dispatch fails closed instead of guessing or answering a
second time.

Quota succession is exposed separately. `kontor_seat_quota_states_list` is an
Observer read joining each live delivery seat to its exact account and provider
quota projections. `kontor_seat_recover` is an Admin, bodyless command addressed
only by project, predecessor run and `Idempotency-Key`; the server fresh-reads
and freezes every binding, revision and quota-provenance fact rather than
accepting an eligibility claim from the caller.

## GitHub App for publication identity (ASMA-8101)

Kontor judges every publication against the confirmed Kontor/Jira binding
(`publication:preview` / `publication:attest`). The forge learns the verdict
only through a GitHub App whose identity cannot bypass the rules it reports on.
The App is optional: without it the attestation endpoints still serve the ASMA
CLI, and nothing is posted to GitHub.

Create the App once per organisation (Settings → Developer settings → GitHub
Apps → New GitHub App) with:

- name `asma-publication-policy`; webhooks disabled (Kontor polls);
- repository permissions: **Checks: read and write**, **Pull requests: read**,
  **Contents: read and write** (the squash merge), **Metadata: read**;
- install it on exactly the governed repositories;
- generate one private key and store the PEM outside every repository, mode
  `0600`, for example `~/.local/state/kontor/asma/config/github-app.pem`.

Then write `<state root>/config/github-app.json`:

```json
{
  "schema_version": 1,
  "app_id": 123456,
  "installation_id": 7890123,
  "private_key_path": "/Users/me/.local/state/kontor/asma/config/github-app.pem",
  "project_id": "01a0064a-e056-7603-9968-ef64fdaacb75",
  "repositories": ["Carasent-ASMA/asma-modules", "Carasent-ASMA/asma-rs-kontor"],
  "check_name": "asma/publication-identity",
  "poll_seconds": 60
}
```

Absence of the document is valid. A present document that will not parse, or
that names a key that will not load, refuses the start: an operator who wrote
it believes the forge check is armed.

With the document in place the daemon:

- polls every open pull request of each listed repository every
  `poll_seconds`, judges its head branch, head commit and title through the same
  attestation the CLI uses, and posts one completed check run named
  `check_name` per judged head/title (`success` or `failure`, with the stable
  reason codes in the summary);
- serves `publication:merge`, which re-reads the pull request, refuses a head
  that moved since it was judged (`head_sha_stale`), re-judges, and squash-merges
  through the App identity bound to that exact head.

Make the check required on the default branch through a repository ruleset
(`required_status_checks` with `integration_id` set to the App id) so a status of
the same name posted by anyone else does not satisfy it. On GitHub Team plans a
branch-name ruleset is not evaluated; this check is the forge-side identity gate.

The repository also ships the dependency-free
`asma/publication-branch-title` Actions check. Require it on the default branch
as the forge-local grammar boundary: it rejects a PR unless the base is
`master`, the branch is canonical, and branch and title carry the same Jira
key. The GitHub App check remains the stronger semantic boundary because it
also proves that key against Kontor's confirmed Jira binding.

## Other deployment data

- Profile packs define phases, gates, artifacts, budgets and runtime routing.
  The bundled manifest declares 17 work-profile categories; four ship today
  (`code`, `ux-ui-layout`, `research`, `docs`).
- Team Definition JSON revisions define native hierarchy, naming, fixed slots,
  delivery `team_slots`, exact labels and slot capability-profile references.
  Team templates and
  consultation profiles separately own execution behavior, skills, context and
  handoffs. Role slots carry stable ids, so two peers in the same role are
  explicit rather than duplicate.
- The standard role catalog defines 56 role codes across 9 segments. Seat
  selection is by `role_code`; a free-form role string is not accepted anywhere.
- Account profiles contain non-secret provider-routing metadata and a credential
  reference. No surface — DTO, row, log, export or process argument — has a field
  for a secret value.
- Completion profiles name the integration team, the verdict committee, the
  number of remediation rounds and an optional polling fallback. The seeded
  `operational_default` allows one remediation round.
- Native container and seat naming is rendered only from the pinned Team
  Definition revision; callers and adapters do not improvise it.

Changing a prompt, duration, template or specification changes configuration.
Changing a safety invariant requires an architectural decision and code review.
