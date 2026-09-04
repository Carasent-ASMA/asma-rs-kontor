# Configuration

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
| `<state-root>/supervision.yml` | Seat supervision policy (optional; see below) |
| `<state-root>/quota-signals.yml` | Vendor exhaustion wording, applied to a seat's own refusal text (optional; see below) |
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
| absent | leaves classification inert; the 300-second poll stays the sole source of truth, exactly as before the file existed |
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
