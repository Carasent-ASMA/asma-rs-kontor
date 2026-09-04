# Provider quota routing: durable state behind the rung walk

Status: **Historical shipped baseline plus a 2026-09-04 local candidate.** The
v48 quota state, v50 live poller and v51 concurrent-window, credit-balance,
account-before-rung and governed-pin work are on `origin/master`. The current
candidate tree adds KON-OP-21 reactive evidence capture, durable succession and
resident bounded recovery, with local tests only. Launch-time
`Wait`/`NeedsHuman` actuation remains open. Merge, independent audit and
live-runtime verification of succession are not claimed here.
Date: 2026-08-21, extended 2026-08-22, candidate status 2026-09-04

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
runtime settings document, resolved once when the daemon composes the adapter.
So the pre-existing
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
  account can still serve. At the v48 checkpoint the measured realm held one
  account profile, so the two coincided, but `any` became wrong as soon as a
  second was registered.

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
- **Automatic handoff with receipts is the required outcome.** At this 2026-08-21
  decision point it was not current behavior; KON-OP-21 now implements the
  candidate path with a durable attempt, redacted handoff, linked successor and
  immutable receipt. Release and live proof remain pending.
- **Seat titles come only from the pinned Team Definition.** Succession retains
  the immutable role slot and therefore its exact configured role-only name,
  for example `SA`, `SWE`, `QA` or `AUD`. Account, provider, rung and Jira key
  remain evidence and routing fields and are never appended to the native title.
- **Any enabled account in the project is eligible for rotation.** Registration
  into a project is the authorization; there is no separate scope field.
  `enabled: false` removes an account permanently and
  `AvailabilityOverride { available: false }` excludes it temporarily.

## Design

### What "account" means here — historical blocker and shipped resolution

Two facts found while implementing, both of which narrow the design:

**At the pre-v51 checkpoint, `harness` was the runtime family rather than the
provider selector.** An `AccountProfile`'s
`harness` is a `RuntimeKindKey` such as `paseo.agent` — not `codex` or `claude`.
The measured realm then held exactly one profile (`Igor · Local Paseo`, `paseo.agent`,
enabled) and all 108 runtime bindings are `paseo.agent`. So under Paseo one
account profile serves *every* provider, and a rung advance from Codex to Claude
does **not** change the account. That is good news for the mechanism: the pin
contract in `admit_pinned_launch` — "the pin is the run's, not the request's",
`LaunchRefusal::PinMismatch` in `kontor-accounts::launch` —
is untouched by a rung advance, so the advance is implementable inside one run.

**At that checkpoint Kontor could not address an individual Codex login.** The
two Codex accounts lived as separate `CODEX_HOME` directories under
`~/.agentsroom/codex-profiles/cxp-*/auth.json`, which is AgentsRoom's mechanism.
But `paseo agent run` as Kontor drives it takes `--workspace --cwd --provider
--model --mode --thinking --title --label` and a positional prompt, and nothing
else (`crates/kontor-runtime-paseo/src/client.rs:220-251`); Paseo's own
`create_agent` API exposes no account or profile parameter either. The adapter
that *does* implement per-account isolation properly —
`kontor-runtime-codex`, which clears and re-resolves `CODEX_HOME` per run — is in
the daemon's `DEFERRED_FAMILIES` list and cannot be
composed by this build.

So per-login rotation ("try the work Codex account, then the personal one") was
**blocked upstream**, not merely unbuilt in that increment. It needed either an
account selector on Paseo's agent-run surface or the Codex family lifted out of
`DEFERRED_FAMILIES`.

> **Superseded 2026-08-22 (v51).** The account selector was already in that flag
> list. `--provider` *is* one, once a deployment registers one provider alias per
> coding account and declares it — see *The upstream blocker is lifted, by
> declaration* below. The paragraph above is kept because its reasoning is what
> the declaration answers.

When a deployment does not make that declaration, Kontor cannot attest a
per-account pin and refuses to claim one. A qualified Paseo deployment declares
one addressable provider alias per account; availability and headroom remain
scoped to `(account_profile_id, provider)` and launch reads the selected provider
alias back before accepting the pin.

The good news is that this does not block the fix that matters. Codex exhausted →
run the seat on Claude is a `--provider`/`--model` change that Kontor fully
controls, and it alone would have kept work moving on 2026-08-21.

### Two events, not one

| Event | Mechanism | Reachable today |
| --- | --- | --- |
| New launch: `rung1 x work` -> `rung1 x personal` | account-before-rung resolver plus a declared provider alias per account | **yes**, when `provider_selects_account` is configured and read back |
| New launch: `rung1` -> `rung2` | the same headroom walk | **yes** |
| Running seat: exhausted account -> successor | runtime refusal detection, redacted handoff and successor-run path | **candidate only** — KON-OP-21 is locally implemented and tested; merge/audit/live verification pending |

### Reuse, do not add

`CapacityObservation` and `AvailabilityOverride`
(`crates/kontor-core/src/repository.rs`) are already per-`account_profile_id`,
already revisioned, and `AvailabilityOverride` is already the operator's standing
judgement with an expiry -- which is the credit-top-up lever for `Drained`. They
are keyed on the account alone, which is why they could not carry this state, but
nothing here duplicates them.

The production Paseo path isolates accounts through one declared provider alias
and credential home per account, then attests the provider id by readback. The
hermetic direct-Codex adapter's `CODEX_HOME` isolation remains useful contract
evidence but is not production-composed; the selector is no longer the blocker.

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

## Resolved after v51: consultation routing

Advisor and Committee seats formerly took `rungs.first()` and ignored the rest
of their declared chain. Current `origin/master` routes both through
`freeze_consultation_model_rung`, which applies the same account-before-rung
headroom resolver used by delivery launches. Consultation launches still share
the launch-time `Wait` / `NeedsHuman` actuation gap documented below; they no
longer ignore fallback rungs.

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

Thresholds are declared per window kind. The resolver returns `Wait` rather
than descending when every account on a rung is blocked and the blocking window
returns inside the declared short horizon. At total exhaustion it computes the
earliest reset or a `NeedsHuman` escalation with an `OP-REQ-036` recommendation
and the walk itself as the deliberation path. The delivery launch boundary does
not yet actuate either outcome into an automatic park or escalation; see *Still
open*.

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

## Remaining and candidate-only work

### Enact launch-time `Wait` and `NeedsHuman`

`kontor_scheduler::headroom::resolve` returns a typed `Placement::Wait` with a
reset instant or `Placement::NeedsHuman` with an escalation payload. The current
delivery launch function must return a `ModelRung`; on those two outcomes it
drops the payload and preserves the adapter's typed provider-outage refusal
path. The resolver is truthful, but the launch path does not yet park the work
until reset or persist the escalation it computed.

Acceptance: a launch-time `Wait` parks without dispatch and wakes at the stored
reset; `NeedsHuman` persists the exact deliberation path and recommendation;
restart/replay reproduces either outcome without inventing a rung or a second
launch.

### Vendor and model tables

Replace the hardcoded catalog in
`Applications::model_catalog`, whose own provenance
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

### Automatic detection — candidate implemented, release pending

The pre-flight half landed in v51. The current KON-OP-21 candidate also filters
the runtime timeline to message events for the exact immutable binding/native
generation, matches only configured provider signals eligible for the seat's
account, and persists redacted runtime-observation quota provenance bound to the
latest blocked cursor. It fails closed on gaps, mixed generations, stale or
foreign provenance and contradictory evidence.

State remains recorded per `(account_profile_id, provider)`, never per provider
alone: with two Codex accounts, "Codex is exhausted" is not a fact that exists.
The parser and persistence path have focused local regressions; a captured live
runtime refusal and exact installed-realm readback are still required release
evidence.

### Persist which rung was chosen

Nothing records which rung a seat actually launched on, so the UI cannot show it
and an operator cannot tell a Codex seat from a Claude one without reading the
runtime. This needs a column on `agent_runs` (`selected_rung_index`, plus a
`selection_reason` of `primary` | `skipped_exhausted` | `operator`) written where
the launch outcome is recorded, and one more migration.

Igor asked for this explicitly as a UI signal; it is the next thing to build.

### Resident supervision and automatic handoff — candidate implemented, release pending

The current candidate records a durable succession attempt before effects,
builds a bounded redacted handoff from the exact binding generation, resolves
account before rung, launches and freshly observes the successor, retires the
predecessor without deleting its runtime-owned history, and confirms one
immutable receipt. The bodyless Admin recovery command and schema-v2 resident
loop both drive that same saga; neither accepts caller-supplied quota authority.

Two hazards to get right:

- **Idempotency.** Durable attempts are keyed to the exact predecessor slot and
  act as both queue and slot lock. Startup, append wake, cadence and a manual
  bodyless replay resume that attempt; they do not mint parallel successors.
- **The successor budget is shared.** `max_successor_depth` also bounds
  hang-recovery replacements, so a team that spent its depth on hangs cannot fall
  back on quota. Keeping one budget is defensible — a seat replaced three times
  needs a human regardless — but the refusal must say which budget was spent.

Local acceptance covers exact provenance, bounded handoff, replay and one
successor under repeated supervision. Release acceptance still requires a live
seat whose provider exhausts mid-turn to recover on the next admissible pair,
retain its exact Team Definition role-only seat name, preserve predecessor
evidence, and produce the same durable receipt when supervision fires again.

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
implementation did not honour. **Both resolver decisions are honoured as of
v51** — see *What schema v51 added* above. The description below is retained as
the rationale, not as current gap status:

* **Account before rung.** "A second account on the same rung costs nothing while
  descending costs quality, so `codex:team` is tried before dropping off
  `codex:prolite`'s rung." Those were the historical aliases; a declared Paseo
  provider alias per account (`codex-work`, `codex-personal`) is the shipped
  selector.
* **Wait rather than descend when the reset is near.** "Step 4's descent is
  skipped entirely when the blocking window resets inside the declared short
  horizon: waiting beats shipping worse work." The resolver makes that choice;
  the launch boundary still needs the actuation work described under *Still
  open* so it can hold the seat instead of merely preserving a refusal.

The policy also records the one provider fact KON-OP-13 still has to establish:
whether `codex-work` exposes its own rate-limit readings from its own effective
home or shares `codex-personal`'s. The practical value of account-before-rung
resolution depends on the answer, though its correctness does not; the
per-account grain of `provider_quota_states` is built to hold either.
