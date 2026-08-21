# Automatic rung and account fallback on provider quota exhaustion

Status: steps 1, 3-partial and 4-partial landed; automatic detection and
live-seat rescue open. A realm can now be told a provider is out and every new
launch routes around it; nothing yet notices on its own.
Date: 2026-08-21
Ticket: **KON-OP-13 / ASMA-7882** owns this. The fleet-mechanics plan
(`_docs/ai-orchestration/plans/2026-08-05-02-37-plan-agent-fleet-mechanics-layer.md`)
was amended on 2026-08-16 to hand Kontor "reserve policy, headroom thresholds,
failover order, `blocked_until` state and the decision of which
account/provider/rung runs next" under that ticket. This document is the
implementation record for the Kontor half; it is not a new plan.

Branch: `feat/ASMA-7869-operational-hardening-and-quota-routing-plan`.

Continues: ASMA-7854 / KON-MVP-25, which built the *authoring* half of the model
chain (`evidence/ASMA-7854-IMPLEMENT-HANDOFF.md`). This is the execution half.

## Two rules inherited from the fleet plan

Both are load-bearing and neither is obvious from inside this repo. The fleet
plan remains the reference for *how to observe a provider*; Kontor consumes those
probes rather than re-deriving them.

**Derive window semantics from the window length, never from a field name.** On
this machine every populated Codex rollout `primary` reads
`window_minutes: 43200` — thirty days, not five hours — and `secondary` is
populated in *zero* of thirty-nine rollout files. A probe that hardcodes
"primary = 5h, secondary = weekly" is confidently wrong the moment a plan
changes. This is why `resets_at` here is parsed from what the provider actually
said and `reset_kind`/`window_seconds` are descriptive columns that routing never
reads.

**DeepSeek is the one provider with a true runtime quota check.**
`GET https://api.deepseek.com/user/balance` returns `total_balance` and
`is_available`, with the key in `~/.local/share/opencode/auth.json`. That is the
concrete collector for the `Drained` state below, and it is the reason `Drained`
is worth modelling separately rather than folded into `Exhausted`.

A third, already satisfied: a Claude limit must block the *whole* Claude
provider, which is what `pooledUsage: true` on Claude encodes.

## The incident this exists for

On 2026-08-21 both Codex accounts hit their plan allowance within a day of each
other. Every seat pinned to Codex stopped with the provider's own text —

> [System Error] You've hit your usage limit. Visit … to purchase more credits or
> try again at Aug 23rd, 2026 9:35 AM.

— and stayed stopped. No seat fell back to a declared lower rung, no seat rotated
to the other account, and no watchdog woke. Work did not resume until the weekly
window reset two days later.

## Why nothing moved

Four independent gaps, only one of which is the rung logic.

1. **`LimitExceeded` is not a quota signal.** `RuntimeError::LimitExceeded`
   (`crates/kontor-runtime/src/adapter.rs:62-69`) is produced in exactly one
   place — `crates/kontor-runtime/src/capability.rs:259` — and means *Kontor's own
   declared request bound was exceeded*. `ProbeRefusal::is_pressure()`
   (`crates/kontor-accounts/src/capacity.rs:93`) is wired to it, so the entire
   pressure → `Cooling` path can never fire on a provider usage limit.
2. **The provider's message never becomes a typed fact.** It arrives as session
   frame text with the agent in `PaseoAgentStatus::Error`
   (`crates/kontor-runtime-paseo/src/wire.rs:510`). Nothing parses it — even
   though the reset instant is *in* the message and is better data than any
   synthetic cooldown.
3. **The watchdog has no eyes.** `SeatObservation`
   (`crates/kontor-daemon/src/supervision.rs:167`) has zero producers in the
   tree, so `WakeCondition::RuntimeError` is declared, validated by
   `SupervisionPolicy::validate`, and never evaluated against anything.
4. **Only rung 1 is ever launched.** `freeze_seat_model_rung`
   (`crates/kontor-daemon/src/applications.rs:4150-4159`) takes
   `chain.rungs.first()`. Rungs 2–4 validate, publish, and are then ignored.

Two further facts shaped the design. `COOLDOWN_SECONDS = 300`
(`crates/kontor-accounts/src/capacity.rs:49`) is the wrong shape for a weekly or
five-day window. And `FailoverReason::AccountExhausted`
(`crates/kontor-accounts/src/launch.rs:389`) — "the account hit a provider quota
or cooldown" — exists with no caller anywhere in cli, api or daemon.

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
`DEFERRED_FAMILIES`. Until then, availability must be scoped
`(account_profile_id, provider)` rather than per account: with one profile
serving every provider, `AvailabilityOverride` — which is per account — cannot
express "Codex is out, Claude is fine", which is precisely the state the incident
left the realm in.

The good news is that this does not block the fix that matters. Codex exhausted →
run the seat on Claude is a `--provider`/`--model` change that Kontor fully
controls, and it alone would have kept work moving on 2026-08-21.

### The account axis sits below the rung

A rung is a capability choice (which model, which effort). An account is a
capacity choice. They are different axes and the account one is resolved
*beneath* the rung:

```
rung 1  codex/gpt-5.6-sol   × account: work → personal
rung 2  claude/claude-opus-5 × account: …
```

Attempt order: exhaust a rung's accounts, then descend. `rung1×work →
rung1×personal → rung2×…`.

This is why the console's `rung_2_same_provider` blocking rule
(`apps/console/src/state/teams.ts:660-687`) stays correct as written: a second
account is never authored as a second rung, so the rule never stands in the way
of the real fallback.

It also gives `pooledUsage` a precise definition rather than a bare bool: **one
quota covers all of a provider's routes, for one account.** That is exactly why
`rung1×personal` is a live fallback while `rung2` on the same exhausted account
is not.

### Two events, not one

| Event | Mechanism | Receipt |
| --- | --- | --- |
| `rung1×work` → `rung1×personal` | `FailoverRequest` / `FailoverReason::AccountExhausted` | account rotation |
| `rung1` → `rung2` | rung advance (new) | rung advance |

The `FailoverReason` enum was built for the first and never called. Only the
second is genuinely new — and per the blocker above, only the second is reachable
today. Account rotation stays specified but unimplementable until Paseo can be
told which login to run as.

### Reuse, do not add

Per-account capacity is already a first-class record. `CapacityObservation`
(`crates/kontor-core/src/repository.rs:1774-1791`) is keyed on
`account_profile_id` and already carries `available`, `pressure` and
`cooling_until`; its raw `reading` is a `CanonicalDocument`, so parsed quota
evidence has a home. `AvailabilityOverride` (`:1815-1838`) is an operator's
standing per-account judgement with a reason and an expiry — which is the
credit-top-up lever, already built and already revisioned. Extend these rather
than adding a limit-state table.

Account isolation is likewise done: the Codex adapter clears `CODEX_HOME` before
every launch so an ambient home cannot be inherited by a run pinned to another
account ("an ambient home is not a fallback; it is the failure this adapter
exists to make impossible", `crates/kontor-runtime-codex/src/adapter.rs:958-961`),
resolves the home only through the account's approved alias, and requires a
non-secret marker file inside it. What is absent is only the *selector* between
two isolated homes.

## Build order

Each step is independently useful. Steps 2–4 stop new work from dying; step 5
rescues a seat that was already mid-turn, which is the specific symptom of the
incident above.

### 1. Correct `pooledUsage` for the pooling providers — **done**

Codex is pooled, on the evidence of the outage: every route died together. Credit
vendors pool by definition — one balance serves every route.

- `crates/kontor-daemon/src/applications.rs` — codex → `true`, with the
  per-account meaning documented.
- `apps/console/src/state/teams.ts` — codex, deepseek and openrouter → `true`.
- `apps/console/src/state/teams.test.ts` — pins the codex catalog value, so a
  revert fails rather than silently re-blessing an unreachable chain.

Cursor is deliberately left alone: `included_usage` against an allowance is
probably pooled too, but there is no evidence for it and a guess here is exactly
what the provenance discipline in this module exists to prevent.

### 1b. Per-provider quota state and the routing that uses it — **done**

Schema v37 adds `provider_quota_states`, keyed
`(project_id, account_profile_id, provider)` — the grain account-scoped
availability cannot express. The `exhausted`/`drained` split is a table CHECK,
not a convention: an exhausted allowance must carry a reset instant and nothing
else may, so a drained credit key cannot be handed to a timer.

The migration deliberately does *not* widen `command_receipts.kind`. That CHECK
now lists over fifty kinds accumulated across v29-v35, and a hand-copied rebuild
to add one value could silently drop any of them; the write rides
`override_availability` and names itself in its intent instead.

`ModelChainPolicy::first_reachable` walks the chain and returns the first rung
whose provider clears, with its index. `select_seat_model_rung` in the daemon
replaces `freeze_seat_model_rung` at all three launch sites. Two asymmetries are
deliberate and tested: absence of a state row *permits* (no evidence is not
`Unknown`, and blocking on absence would stop every launch in a realm that has
never recorded one), and a provider is held back only when *all* states for it
block — one exhausted account does not exhaust a provider another account can
serve. `None` from the walk is a refusal, never a silent return to rung 1.

An operator surface lands with it: `kontor provider-quota-record` and
`kontor provider-quota-states-list`. Recording is admin, because whoever can
write these states can route every seat in the realm by declaring the rest
exhausted; reading is observer.

Verified: 110 suites / 1444 tests green workspace-wide, console 279 green,
`tsc --noEmit` clean, contract document and console types regenerated.

### 2. Vendor and model tables

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

### 3. `ProviderQuotaExhausted` — automatic detection, still open

A **new** `RuntimeError` variant carrying the provider and the parsed
`LimitState`. Deliberately separate from `LimitExceeded` — conflating a provider
quota with Kontor's own request bound is gap 1 above. `ProbeRefusal` gains the
matching token and `is_pressure()` moves to it. Retires `COOLDOWN_SECONDS`.

State is recorded per `(account_profile_id, provider)`, never per provider alone:
with two Codex accounts, "Codex is exhausted" is not a fact that exists.

Acceptance: the Codex message in the incident above parses to
`Exhausted { resets_at: 2026-08-23T09:35 }` against the seeded pattern; a 402
with no date parses to `Drained`; `Drained` yields no requeue instant.

### 4. Persist the selection — partially done

The walk itself landed in 1b. What remains is *persisting* the outcome: the
selector returns the rung index and the daemon currently drops it, so the UI
cannot yet show which rung a seat is on. That needs a column on `agent_runs`
(`selected_rung_index`, plus a `selection_reason` of `primary` |
`skipped_exhausted` | `operator`) and one more migration.

Also still open: refusing with the earliest recovery instant rather than a bare
refusal, so the scheduler requeues at the reset rather than dropping the work.

Every pair unavailable is still a refusal — but it carries the earliest recovery
instant among them so the scheduler requeues then, or reports "no pair
recoverable" when they are all `Drained`. *Waiting until 09:35* and *stuck* are
different states and only one of them needs a human.

Also add the validation that warns when a chain's provider has fewer than two
enabled accounts in the project: account profiles are project-scoped
(`list_account_profiles(project_id)`), so an account registered in one project is
not a fallback in another, and unconfigured would otherwise look like broken.

Acceptance: a three-rung chain with rung 1 exhausted and rung 2 drained selects
rung 3; the run row states which pair won and why; an all-exhausted chain returns
the earliest reset rather than a bare refusal.

### 5. Watchdog producer and automatic handoff

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

## Open question

Correcting `codex.pooledUsage` in step 1 promotes the seeded standard-builder
chain from a visible notice to unpublishable — see `SEED_TEAMS` at
`apps/console/src/state/teams.ts:1590`, whose contract is that every chain is
"copied rung-for-rung from the fleet policy, and the copying is the point". The
policy really does place two Codex rungs together. With Codex correctly pooled,
that is not a deviation worth flagging, it is a fallback that cannot fire.

So either the fleet policy is amended and the seed follows, or the seed keeps
mirroring the policy verbatim and `seeds no draft that could not be published`
(`apps/console/src/state/teams.test.ts:1145`) has to become a weaker claim. This
needs whoever owns the fleet policy document; it is not a call to make inside the
console fixture.

Related consequence worth noting: with every real vendor pooling except Cursor,
the `provider_repeat` *notice* severity becomes nearly unreachable in practice.
Only a Cursor repeat can produce one.
