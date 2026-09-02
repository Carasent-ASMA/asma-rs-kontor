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
| `<state-root>/runtimes.json` | Runtime family, plane endpoint and per-account provider aliases. Schema generation `4`; generation `3` is refused rather than upgraded, because it can compose the right sessions under misleading names |
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
5. Reconcile every legacy ticket TSW through `topology:materialize` using its
   stable historical key where available. The selected/pinned definition maps
   each open TeamRun's exact slot to one logical SeatBinding without creating or
   replacing a native session. Replay it again to prove the same binding ids.
6. Preview the existing epic's Team Definition upgrade. Confirm the complete
   identity-bound container-and-seat census before apply. Preview first
   preflights every exact slot of every live TeamRun against the target
   definition and performs no runtime read when a mapping is missing or two
   co-resident slots would render the same name.
7. Apply with one stable idempotency key. A partial result keeps the old pin and
   fences materialization; replay the same key until every exact native object
   reads back and the pin switches. The fence blocks admission and replacement
   before any command write, predecessor retirement or runtime contact.

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
