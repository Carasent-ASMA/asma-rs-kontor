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
| `<state-root>/runtimes.json` | Runtime family, plane endpoint and per-account provider aliases. Schema generation `4`; generation `3` is refused rather than upgraded, because it can compose the right sessions under misleading names |
| `<state-root>/supervision.yml` | Seat supervision policy (optional; see below) |
| `<state-root>/quota-signals.yml` | Vendor exhaustion wording, applied to a seat's own refusal text (optional; see below) |
| `<state-root>/credentials.json` | The realm's three tier secrets, `0600` |
| `<state-root>/endpoint.json` | Where the realm listens, when not on the default loopback port |
| `<state-root>/provider-homes/` | One credential home per provider account — `CODEX_HOME` for Codex, `CLAUDE_CONFIG_DIR` for Claude |
| `crates/kontor-mcp/seats/*.json` | Which tier and serve profile one MCP server process runs at |

Everything in the database is published through a preview/apply pair with a
content hash: the apply is compared against the hash the preview returned, so a
specification cannot change between the two.

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
