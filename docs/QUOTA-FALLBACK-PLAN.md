# Provider quota routing: durable state behind the rung walk

Status: v48's rung walk landed 2026-08-21. **Schema v51 extends it to concurrent
windows, a credit balance and account-before-rung resolution (2026-08-22, this
branch), after master's v49 command-execution-mode and v50 live quota poller;**
the remaining open items are listed under *Still open*.
Date: 2026-08-21, extended 2026-08-22

Ticket: **KON-OP-13 / ASMA-7882** owns this. The fleet-mechanics plan
(`_docs/ai-orchestration/plans/2026-08-05-02-37-plan-agent-fleet-mechanics-layer.md`)
was amended on 2026-08-16 to hand Kontor "reserve policy, headroom thresholds,
failover order, `blocked_until` state and the decision of which
account/provider/rung runs next" under that ticket. This document is the
implementation record for the Kontor half; it is not a new plan.

Continues: ASMA-7854 / KON-MVP-25, which built the *authoring* half of the model
chain (`evidence/ASMA-7854-IMPLEMENT-HANDOFF.md`).

## The incident

On 2026-08-21 both Codex accounts hit their plan allowance within a day of each
other. Every seat pinned to Codex stopped with the provider's own text --

> [System Error] You've hit your usage limit. Visit ... to purchase more credits
> or try again at Aug 23rd, 2026 9:35 AM.

-- and stayed stopped until the weekly window reset two days later.

## What already existed, and what was actually missing

`freeze_seat_model_rung` already walks the chain and takes the first rung whose
provider clears `adapter.provider_available`, with an adapter-supplied fallback
route behind it. The rung ladder was **not** inert.

What it consults is the gap. `PaseoAdapter::provider_available` reads
`PaseoAdapterConfig.unavailable_providers` -- a `#[serde(default)]` field on the
runtime settings document, resolved once when the adapter is composed
(`crates/kontor-daemon/src/runtimes.rs:211-216`, `:491`). So the pre-existing
operator lever was: edit the settings file, restart the daemon. That set

* needs a restart to change,
* applies to every account at once, and
* never stops being true -- an allowance that returns on Saturday keeps blocking
  until a human remembers to delete the entry.

None of those are survivable for a limit that resets on a clock, which is why the
outage needed a human in the loop at all.

## What this change adds

Schema v48 `provider_quota_states`, keyed
`(project_id, account_profile_id, provider)`, read at every launch and OR-ed into
the availability decision beside the adapter's own answer. Both are consulted and
either can hold a rung back: an operator who excluded a provider in settings does
not want a stored row overriding them, and a provider observed out of quota is
out whatever the settings say.

`QuotaOutlook::blocks` in the daemon holds two deliberate asymmetries:

* **Absence permits.** No row is not the same fact as `ProviderQuotaKind::Unknown`,
  which is an explicit refusal. Blocking on absence would stop every launch in a
  realm that has never recorded a state -- which is every realm before the first
  collector run.
* **`all`, not `any`.** One exhausted account does not exhaust a provider another
  account can still serve. The realm holds one account profile today so the two
  coincide, but `any` becomes wrong the moment a second is registered.

The `exhausted`/`drained` split is a table CHECK rather than a convention: an
exhausted allowance must carry a reset instant and nothing else may. A plan
allowance recovers on a clock and stops blocking on its own; a credit balance
recovers only on payment and must never be handed to a requeue timer, because
that is a retry loop against a dead key.

**Master's refusal shape was kept.** The walk still ends in
`.or_else(|| chain.rungs.first().cloned())`, preserving the frozen primary when
every route is held back, so the adapter emits its typed provider-outage refusal
rather than the daemon inventing a substitute. An earlier draft of this change
refused inside the daemon instead; master's version is better and this one
adopts it.

## Decisions

Taken 2026-08-21 with Igor. These are settled; revisit them explicitly rather
than by drift.

- **Credit-balance vendors are in scope from the first cut.** So exhaustion is
  not one state: `Available | Exhausted { resets_at } | Drained | Unknown`. A
  plan allowance recovers on a clock; a credit balance recovers on money.
  `Drained` must **never** be requeued on a timer — retrying a dead OpenRouter
  key every five minutes forever is a new failure mode, not a fallback.
- **Handoff is fully automatic, with receipts.** The 3am save is the point. Every
  hop writes a command receipt and a `parent_agent_run_id` link.
- **Seat titles reuse the existing suffix segment**, e.g.
  `ARCHITECT · ASMA-7854 · R1/work`. The account appears as its profile *label*,
  never as the account email — this codebase deliberately keeps provider identity
  out of displayed and persisted text (`FailoverReason` carries no free-text
  note; `ensure_account_profile` stores a `credential_alias_digest`).
- **Any enabled account in the project is eligible for rotation.** Registration
  into a project is the authorization; there is no separate scope field.
  `enabled: false` removes an account permanently and
  `AvailabilityOverride { available: false }` excludes it temporarily.

## Design

### What "account" means here — and the upstream blocker

Two facts found while implementing, both of which narrow the design:

**`harness` is the runtime family, not the provider.** An `AccountProfile`'s
`harness` is a `RuntimeKindKey` such as `paseo.agent` — not `codex` or `claude`.
The live realm holds exactly one profile (`Igor · Local Paseo`, `paseo.agent`,
enabled) and all 108 runtime bindings are `paseo.agent`. So under Paseo one
account profile serves *every* provider, and a rung advance from Codex to Claude
does **not** change the account. That is good news for the mechanism: the pin
contract in `admit_pinned_launch` — "the pin is the run's, not the request's",
`LaunchRefusal::PinMismatch` (`crates/kontor-accounts/src/launch.rs:276-282`) —
is untouched by a rung advance, so the advance is implementable inside one run.

**Kontor cannot currently address an individual Codex login.** The two Codex
accounts live as separate `CODEX_HOME` directories under
`~/.agentsroom/codex-profiles/cxp-*/auth.json`, which is AgentsRoom's mechanism.
But `paseo agent run` as Kontor drives it takes `--workspace --cwd --provider
--model --mode --thinking --title --label` and a positional prompt, and nothing
else (`crates/kontor-runtime-paseo/src/client.rs:220-251`); Paseo's own
`create_agent` API exposes no account or profile parameter either. The adapter
that *does* implement per-account isolation properly —
`kontor-runtime-codex`, which clears and re-resolves `CODEX_HOME` per run — is in
`DEFERRED_FAMILIES` (`crates/kontor-daemon/src/runtimes.rs:61`) and cannot be
composed by this build.

So per-login rotation ("try the work Codex account, then the personal one") is
**blocked upstream**, not merely unbuilt here. It needs either an account
selector on Paseo's agent-run surface or the Codex family lifted out of
`DEFERRED_FAMILIES`.

> **Superseded 2026-08-22 (v51).** The account selector was already in that flag
> list. `--provider` *is* one, once a deployment registers one provider alias per
> coding account and declares it — see *The upstream blocker is lifted, by
> declaration* below. The paragraph above is kept because its reasoning is what
> the declaration answers.

Until the declaration is made, availability is scoped
`(account_profile_id, provider)` rather than per account: with one profile
serving every provider, `AvailabilityOverride` — which is per account — cannot
express "Codex is out, Claude is fine", which is precisely the state the incident
left the realm in.

The good news is that this does not block the fix that matters. Codex exhausted →
run the seat on Claude is a `--provider`/`--model` change that Kontor fully
controls, and it alone would have kept work moving on 2026-08-21.

### Two events, not one

| Event | Mechanism | Reachable today |
| --- | --- | --- |
| `rung1 x work` -> `rung1 x personal` | `FailoverRequest` / `FailoverReason::AccountExhausted` | **yes as of v51**, where the deployment declares `provider_selects_account` and one alias per login |
| `rung1` -> `rung2` | the chain walk, now reading stored state | yes |

`FailoverReason::AccountExhausted` (`crates/kontor-accounts/src/launch.rs`) was
built for the first and still has no caller anywhere in cli, api or daemon.

### Reuse, do not add

`CapacityObservation` and `AvailabilityOverride`
(`crates/kontor-core/src/repository.rs`) are already per-`account_profile_id`,
already revisioned, and `AvailabilityOverride` is already the operator's standing
judgement with an expiry -- which is the credit-top-up lever for `Drained`. They
are keyed on the account alone, which is why they could not carry this state, but
nothing here duplicates them.

Account isolation is likewise done: the Codex adapter clears `CODEX_HOME` before
every launch so an ambient home cannot be inherited by a run pinned to another
account, resolves the home only through the account's approved alias, and
requires a non-secret marker file inside it. What is absent is only the
*selector* between two isolated homes -- see the blocker above.

## What landed

* `codex.pooledUsage` -> true on the outage's own evidence, every Codex route
  having died together; DeepSeek and OpenRouter follow definitionally, one credit
  balance serving every route. Cursor is deliberately left alone: `included_usage`
  against an allowance is probably pooled too, but there is no evidence and a
  guess is what the provenance discipline in that module exists to prevent.
* Schema v48 `provider_quota_states` with the reset-instant CHECK.
* `QuotaOutlook` consulted beside `adapter.provider_available` at all three
  delivery-seat launch sites.
* `kontor provider-quota-record` (admin) and `provider-quota-states-list`
  (observer).
* One incidental root-cause fix: the `operational_hardening_lineage` branch of
  the migration runner ran a hardcoded index list that ended at whatever
  migration existed when it was written, so appending v48 left that one lineage a
  version short and `verify_applied` refused the open. It is now a skip-set over
  `MIGRATIONS`, which cannot fall behind.

## Knowingly not fixed

**Advisor and Committee consultation seats still take `rungs.first()` with no
fallback at all** -- `profile.models.rungs.first()` and
`slot.models.rungs.first()` in `crates/kontor-daemon/src/applications.rs`. Same
defect, different chain source: consultations carry their own `models` rather
than a template slot's `model_chain`, so there is no shared root to fix once.
They were left out to keep this change to the path the outage actually took.

## What schema v51 added (2026-08-22)

This is the OP-REQ-042/043 half. The four items the previous pass listed under
*Also worth knowing* as "settled but not honoured" are now honoured.

### Windows are a set, and blocking takes the latest reset

`provider_quota_states` gained a companion `provider_quota_windows` table keyed
`(project, account, provider, kind)`. One `resets_at` could not describe an
account holding two allowances at once — the Claude plan was verified on
2026-08-14 exposing a five-hour `session` window *and* a weekly one — and the
instant such an account becomes usable is the **latest** reset among the spent
windows, not the earliest. The earliest unblocks it while a window it also needs
is still empty, which walks straight back into the limit just recorded.

`kind` is classified from `window_minutes` and never from the slot the reading
arrived in. The vendor publishes `primary`/`secondary`; those are its layout, not
its meaning, and a reader that trusted them recorded a weekly allowance as
whatever `primary` meant that quarter.

### Credit is the other dimension, and they never touch

The header row gained a balance, its reserve, and **one** shared currency column.
Windows and credit are never converted into each other — verified 2026-08-14, the
Claude org's `used_credits` did not move while a session window climbed 11% ->
28%, so included windows are free and are meant to be spent to the limit while
the credit is the guarded number. Currencies are never converted either, and one
currency column makes an EUR-balance-against-a-USD-floor row unwritable rather
than merely discouraged.

### `cannot_report` is the fifth state, and it is not `unknown`

Both describe an absence of numbers and they are opposite instructions.
`unknown` means *this reading failed* and fails closed. `cannot_report` means
*this provider has no such number to give* — OpenRouter's `:free` routes under
FND-005/DEC-001 — and is used reactively: run until refused, then record the
stated reset. Failing closed on the second retires a provider permanently on the
strength of a figure it was never going to produce.

### Account before rung

`kontor_scheduler::headroom::resolve` walks the accounts eligible for the current
rung before taking the next rung. A second account on the same rung costs
nothing; descending costs quality on every turn that follows. This is also what
makes a four-rung chain reach its fourth rung — the previous selection took the
first clear rung and otherwise fell back to the frozen primary, so rungs three
and four were decoration.

Thresholds are declared per window kind, and a rung whose accounts are all
blocked is *waited for* rather than descended around when the blocking window
returns inside the declared short horizon. Total exhaustion parks until the
earliest reset; a human is reached only past the escalation horizon, carrying an
`OP-REQ-036` recommendation and the walk itself as the deliberation path.

Every threshold gates the admission of a **new** seat. `Placement` has no variant
that can name a running seat, so "pre-empt the seat using the quota" is not a
decision the type can express.

### The upstream blocker is lifted, by declaration

The previous pass recorded per-login rotation as *blocked upstream*: Paseo takes
`--provider` and exposes no account parameter. The lever was already in that flag
list. A deployment that registers **one provider alias per coding account** makes
`--provider` the account selector, so:

* `PaseoConfig.provider_selects_account` declares it, and `account_env` reports
  it rather than being hardcoded `false`;
* the pin is attested by the readback the adapter already performs —
  `verify_agent_route` fails correlation when the provider Paseo reports is not
  the provider that was requested;
* each account profile's immutable `routing` document lists the aliases it is
  addressable under, so an account naming none is simply not walked.

It is **declared, never inferred.** Kontor cannot tell by looking whether two
aliases are two logins or two spellings of one, and guessing permissively would
report a per-run account guarantee the runtime does not make.

### The pre-flight probe

`kontor_runtime_codex::usage` reads `GET https://chatgpt.com/backend-api/wham/usage`
with the token from **that account's own** `CODEX_HOME/auth.json`, and classifies
windows from the structured reset instant — never from refusal prose. Verified
2026-08-05: the same refusal reading *"try again at Aug 30th, 2026 11:28 PM"*
carried `resets_at: 1788121720`, which is `2026-08-30T20:28:40Z`, one timezone
offset apart.

This is the one module in that crate that opens a credential, and it is fenced
the way `kontor-accounts` fences a resolved one: `SecretString`, no `Serialize`,
redacted `Debug`, one exit that builds one header, and every failure mapped to a
closed reason so no path, host or token reaches an error. The adapter's "never
opens `auth.json`" rule is unchanged — it is a rule about the *launch* path,
where nothing needs the token.

**It is also the instrument for the open provider question.** Whether
the personal Codex login reports its own rate limits or shares the work
account's is answered by pointing this probe at each home in turn. The mechanism now keeps them apart;
the reading itself has not been taken.

### Budgets stop being a per-task money ceiling

`execution:arm`'s `budget` is optional and defaults from the epic's **pinned**
work profile, read from a task's frozen workflow snapshot rather than the profile
catalog — the snapshot is what that epic's gates are already judged against, and
a later revision must not re-grade a grant. Explicit bounds may only narrow, and
a stored `ExecutionAuthorization` now reports the bounds it was granted under.

## Still open

### Vendor and model tables

Replace the hardcoded catalog at
`crates/kontor-daemon/src/applications.rs:4883-4947`, whose own provenance
already reads `"state": "fixture/needs-verification"`.

```
provider:  id, label, charging_basis, pooled_usage, reset_kind,
           window_seconds, quota_pattern, reset_capture
model:     id, provider, label, is_default, context_window,
           efforts, pricing, degraded_lane
```

`CapacityObservation` gains `state` and `resets_at`. `reset_kind` and
`window_seconds` (5h = 18000, 5d = 432000, weekly = 604800; null for credit) are
for forecasting and UI copy; `state` is what routing reads.

`quota_pattern` and `reset_capture` are columns, not Rust constants, because
every vendor words exhaustion differently and Codex will reword it next quarter —
a regex in code means a rebuild to track a copy change. One pattern per vendor
suffices: a captured date yields `Exhausted { resets_at }`, a match with nothing
captured yields `Drained`.

Acceptance: the catalog endpoint serves stored rows; no provider or model
identity remains a literal in `applications.rs`; a seeded row per vendor Kontor
actually reaches.

### Automatic detection

**The pre-flight half landed in v51** — `kontor_runtime_codex::usage` asks a
Codex account how much is left before a seat stops on it. What remains open is
the *reactive* half below: turning a refusal a running seat hit into a recorded
state without a human pasting it.

A **new** `RuntimeError` variant carrying the provider and the parsed
`LimitState`. Deliberately separate from `LimitExceeded` — conflating a provider
quota with Kontor's own request bound is gap 1 above. `ProbeRefusal` gains the
matching token and `is_pressure()` moves to it. Retires `COOLDOWN_SECONDS`.

State is recorded per `(account_profile_id, provider)`, never per provider alone:
with two Codex accounts, "Codex is exhausted" is not a fact that exists.

Acceptance: the Codex message in the incident above parses to
`Exhausted { resets_at: 2026-08-23T09:35 }` against the seeded pattern; a 402
with no date parses to `Drained`; `Drained` yields no requeue instant.

### Persist which rung was chosen

Nothing records which rung a seat actually launched on, so the UI cannot show it
and an operator cannot tell a Codex seat from a Claude one without reading the
runtime. This needs a column on `agent_runs` (`selected_rung_index`, plus a
`selection_reason` of `primary` | `skipped_exhausted` | `operator`) written where
the launch outcome is recorded, and one more migration.

Igor asked for this explicitly as a UI signal; it is the next thing to build.

### Watchdog producer and automatic handoff

Produce `SeatObservation` from the adapter (gap 3), adding
`quota_exhausted: Option<LimitState>` and a `WakeReason::QuotaExhausted`. On that
wake, rotate the account first and descend a rung second.

`replace_seat` (`crates/kontor-daemon/src/applications.rs:9444-9560`) already
does the rest: it retires the predecessor, requires `TerminalOutcome::Cancelled`
with `TerminalEvidenceSource::RuntimeObservation` — a quota-dead session
qualifies, being runtime-observed — links `parent_agent_run_id`, bounds hops by
`max_successor_depth`, and refuses on a terminal team run. Retired is not
deleted: the predecessor keeps its transcript and its terminal evidence.

Two hazards to get right:

- **Idempotency.** Watchdog cadence is seconds and `replace_seat` is async.
  Without a deterministic key two ticks create two successors, and
  `allow_duplicate_seat: false` is enforced by policy *validation*, not by
  construction, so it will not save you. Derive the key from
  `(predecessor_agent_run_id, binding_generation)`; a double fire then replays
  the original receipt through the existing branch at `applications.rs:9514`.
- **The successor budget is shared.** `max_successor_depth` also bounds
  hang-recovery replacements, so a team that spent its depth on hangs cannot fall
  back on quota. Keeping one budget is defensible — a seat replaced three times
  needs a human regardless — but the refusal must say which budget was spent.

Acceptance: a seat whose provider exhausts mid-turn is replaced by a successor on
the next available pair, titled with its rung and account; the predecessor is
retired with its evidence intact; a watchdog firing twice produces exactly one
successor.

## Resolved: the fleet policy's adjacent Codex rungs

Correcting `codex.pooledUsage` proved the standard builder's rung 3 unreachable —
the policy placed `codex/gpt-5.6-luna` and `codex/gpt-5.6-terra` adjacently, and
one Codex allowance serves both, so whatever blocked the second blocked the third.

Fixed in the policy on 2026-08-21 by **reordering rather than re-pinning**:
Terra moves from rung 3 to rung 4 and Nemotron takes rung 3. Section 3 of that
document requires a calibration ticket before any seat moves to a different
model, and reordering four existing pins moves none of them. Three of the four
rungs are now reachable during a Codex outage where two were before. The console
seed follows it rung for rung again, as its contract requires.

The precedent was already in the document: the Committee Judge chain carries the
note "while Codex is blocked, rungs a and c are the same pool — skip to d rather
than walking into the same wall". The policy understood pooled fallback; the
builder chain had simply not been checked against it.

## Also worth knowing

The policy's own KON-OP-13 amendment (2026-08-16) settled two things the v48
implementation did not honour. **Both are honoured as of v51** — see *What schema
v51 added* above; the description below is kept for the reasoning:

* **Account before rung.** "A second account on the same rung costs nothing while
  descending costs quality, so `codex:team` is tried before dropping off
  `codex:prolite`'s rung." That is the design recorded above, and it is blocked
  upstream on Paseo exposing an account selector.
* **Wait rather than descend when the reset is near.** "Step 4's descent is
  skipped entirely when the blocking window resets inside the declared short
  horizon: waiting beats shipping worse work." The walk here descends
  immediately. Honouring this needs the short horizon as configuration and a
  scheduler that can hold a seat rather than route it — genuinely open work, and
  it is the difference between routing around an outage and routing around a
  five-minute blip.

The policy also records the one provider fact KON-OP-13 still has to establish:
whether `codex:team` exposes its own rate-limit readings from its own effective
home or shares `codex:prolite`'s. Account-before-rung resolution depends on the
answer, and the per-account grain of `provider_quota_states` is built to hold
either.
