# ASMA-7882 / KON-OP-13 — architect handoff

**Branch:** `feat/ASMA-7882-quota-headroom-routing` (from `origin/master` @ `7dc6212`)
**PR:** https://github.com/Carasent-ASMA/asma-rs-kontor/pull/84
**Date:** 2026-08-22
**Ticket:** ASMA-7882 / `KON-OP-13`; Kontor task `01a027d5-2835-7712-a96f-0a1003e4ac4b`, TeamRun `01a027d7-7ab1-7833-b5e8-37577b4479bb`, TSW `wks_b028bbccef6ad780`

Route work by provider quota headroom instead of a money ceiling, per OP-REQ-042
and OP-REQ-043. Design record: `docs/QUOTA-FALLBACK-PLAN.md`, section *What
schema v49 added (2026-08-22)*.

## Why this seat exists

The template launched a Codex architect. Both Codex accounts were already
recorded exhausted and blocking — until 2026-08-22T17:54:03Z (work) and
2026-08-23T07:35:19Z (personal) — and Codex then refused with a usage limit.
Admin seat-replace onto Claude returned 503 three times (`the session this seat
holds has not been observed finished`). This is the bounded Claude successor in
the same TSW.

**That incident is this ticket's own acceptance counterexample.** Recorded
exhausted-and-blocking state still launched `gpt-5.6-sol`, because nothing
consulted it at launch time in a way that could pick a different account. What
landed here is the mechanism that would have refused that launch.

## What landed

| Deliverable | Where |
| --- | --- |
| Quota-window + credit state | `kontor-core/src/quota.rs`, `repository.rs`, migration `0049` |
| Headroom predicate, latest-reset derivation | `kontor-core/src/quota.rs`, `ProviderQuotaState::headroom` |
| Account-before-rung resolution | `kontor-scheduler/src/headroom.rs` |
| Launch-time rung selection | `kontor-daemon/src/applications.rs` — `freeze_seat_model_rung` |
| `execution:arm` budget default | `kontor-daemon/src/applications.rs` — `armed_budget` |
| Governed pin (`account_env` + `--provider` alias) | `kontor-accounts/src/pin.rs`, `kontor-runtime-paseo/src/adapter.rs` |
| Codex usage observer | `kontor-runtime-codex/src/usage.rs` |

Nine load-bearing decisions, each stated at its own definition site:

1. **Windows are a set; blocking takes the *latest* reset.** One `resets_at`
   cannot describe an account holding a five-hour session window *and* a weekly
   one (verified 2026-08-14). The earliest reset unblocks an account whose other
   window is still empty.
2. **`kind` comes from `window_minutes`, never from the `primary`/`secondary`
   slot.** Those names are the vendor's layout, not its meaning.
3. **Windows and credit never convert, and currencies never compare.** One
   shared `credit_currency` column makes an EUR-balance-against-a-USD-floor row
   unwritable rather than merely discouraged.
4. **`cannot_report` is a fifth state, not a spelling of `unknown`.** `unknown`
   means *this reading failed* and fails closed; `cannot_report` means *this
   provider has no such number* and is used reactively.
5. **Account before rung.** A second account on the same rung costs nothing;
   descending costs quality on every later turn. This is also what makes a
   four-rung chain reach its fourth rung — the previous selection took the first
   clear rung and otherwise fell back to the frozen primary.
6. **A near reset is waited for, not descended around.** Minutes of delay on the
   pinned model beat a whole task's output from a worse one.
7. **Total exhaustion parks; a human is reached only past the horizon**, and then
   with an OP-REQ-036 recommendation whose deliberation path is the walk itself.
8. **Thresholds gate admission only.** `Placement` has no variant that can name a
   running seat, so "pre-empt the seat using the quota" is not expressible.
9. **The governed pin is declared, never inferred.** Kontor cannot tell whether
   two provider aliases are two logins or two spellings of one; guessing
   permissively would attest to a per-run account guarantee Paseo does not make.

## The upstream blocker is lifted

The previous increment recorded per-login rotation as *blocked upstream*: Paseo
takes `--provider` and exposes no account parameter. The selector was already in
that flag list. A deployment that registers **one provider alias per coding
account** and sets `provider_selects_account` makes `--provider` the account
selector; `account_env` then reports it instead of being hardcoded `false`, and
the pin is attested by the provider readback `verify_agent_route` already
performs.

## Verification

Full workspace suite green. New coverage:

- 9 property tests over the multi-window truth table, latest-reset derivation,
  credit/window dimension independence and currency refusal
  (`kontor-core/tests/quota_headroom.rs`).
- 26 resolution tests including account-before-rung, fourth-rung reachability,
  short-horizon wait, three-state observation, control-plane reserve, park vs
  escalate, and the two negative pre-emption proofs
  (`kontor-scheduler/src/headroom.rs`).
- 18 observer tests, 5 of them against a recorded server proving the bearer
  header, per-home account separation, and that a transport fault is never
  reported as an empty payload.
- Store round-trips proving every window, balance, reserve and the
  `cannot_report` state survive a restart.

**Mutants seeded and confirmed killed** (each reverted after):
earliest-instead-of-latest reset; credit/window conversion; currency conversion;
rung-before-account descent; first-rung-only selection; cannot-report collapsed
into unknown; ignored control-plane reserve; escalate-instead-of-park;
short-horizon wait removed; window-set merge instead of replace; classify-by-slot;
round-consumption-down.

## Remaining risks

1. **`execution:arm` now refuses a cross-currency ceiling.** This is the intended
   OP-REQ-042 behaviour and it caught eight stale fixtures whose profile declares
   EUR while the call stated NOK. Any real caller doing the same will start
   getting `placement_blocked`. It is a breaking change for those callers, and it
   is deliberate — the alternative is comparing two currencies as two numbers.
2. **Parking is decided but not yet enacted at launch time.** `resolve` returns
   `Wait`/`NeedsHuman` with the instant and the escalation payload;
   `freeze_seat_model_rung` must return a rung, so on those paths it preserves
   master's refusal shape (frozen primary → the adapter's typed provider-outage
   refusal) and drops the instant. Actually holding the seat needs a launch path
   that can park rather than route. Marked with a `ponytail:` comment at the
   drop site.
3. **The control-plane reserve is exercised at the predicate, not end to end.**
   The three launch sites are delivery seats by construction (each launches task
   work under a TeamRun); ECP control seats are created without a task or
   TeamRun and do not pass through rung selection at all today. The reserve
   protects them by holding headroom back from delivery, which is testable and
   tested — but no ECP seat has yet been admitted through `SeatClass::ControlPlane`
   in a live realm.
4. **No headroom policy is configured anywhere yet.** `CapacityConfig.headroom`
   is `Option` and defaults to `None`, which resolves under
   `HeadroomConfig::state_only()` — thresholds at 100, no reserve, immediate
   descent. That is deliberately the pre-existing behaviour, so **this change is
   inert until a deployment declares thresholds.** Declaring them is the next
   operational step, and the fleet policy's §10 numbers (Codex 70% of the weekly
   window; Claude windows spent to the limit) are the intended first values.
5. **The open provider fact is now answerable but unanswered.** Whether
   the personal Codex login reports its own rate limits or shares the work
   account's is settled by pointing the observer at each `CODEX_HOME` in turn.
   (The fleet policy still spells these `codex:team` / `codex:prolite`; commit
   #78 on this ticket renamed the ids to `codex-work` / `codex-personal`,
   because an account is named for the identity it holds and not for its
   billing tier.) The mechanism
   keeps them apart; nobody has taken the reading. Account-before-rung
   resolution's *value* depends on the answer, though its correctness does not.
6. **The observer has no scheduled caller.** It is a library seam
   (`CodexUsageProbe`) with a live implementation and no poller wired to it, so
   window rows are still written only by `provider-quota-states:record`. Wiring a
   collector is out of this ticket's scope (OP-REQ-042 owns the semantics; the
   probe knowledge stays with the fleet-mechanics plan).
7. **`OP-13` was never added to the Kontor epic task graph.** The plan's §5
   *Pending action* still stands: `epics:apply` replaces the whole graph
   atomically and the `TPM` holds that baton. This branch did not touch it.

## Not done, deliberately

`OP-10` was not started and `OP-15` was not taken over, per the brief. The
reactive half of automatic detection — a `RuntimeError` variant carrying the
parsed provider limit, retiring `COOLDOWN_SECONDS` — remains open and is
described in `docs/QUOTA-FALLBACK-PLAN.md` under *Still open*.

## Recovery commit

Claude Opus seat `b5590e38` hit the individual spend / 5h limit after
`cargo test --workspace` finished **1533 passing, 0 failing**. Recovery
architect committed the already-staged tree (no design rewrite) and opened
the PR. Superproject gitlink stays at `7dc6212`.

- Work HEAD: `b8c43b98b277132d4965754098476aa03b781a32`
- PR: https://github.com/Carasent-ASMA/asma-rs-kontor/pull/84
