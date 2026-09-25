# OG-061 — settlement 503s for completed verifier sessions

Receipt `01a0af48-f629-7602-90f6-e66844cd749c`. Owned under ASMA-8203, carried on
the combined integration head.

## Status

The remaining post-deploy failure is diagnosed, reproduced at the adapter
boundary, and corrected. **OG-061 is not closed here**: the correction is
committed and unreleased, and the two preserved tuples need one operational step
that root owns (below). No turn was settled, no gate recorded, nothing deployed.

## Symptom after the combined deployment

Retries under the original idempotency keys returned `503 proof_scan_incomplete`
with `timeline_refetch_required: the session's content must be read again from
the runtime` underneath in the daemon log, while a direct Paseo
`get_agent_activity` read both native sessions completely (updateCount 263 and
296). The plane was healthy and the sessions were readable throughout.

## Root cause

The Kontor epoch number in a proof tuple is not a property of the session. It is
the adapter's *global* allocation index — the n-th distinct raw Paseo epoch this
process has ever seen — handed out by `EpochRegistry::resolve` in first-sight
order across every session.

Migration `0100_runtime_timeline_epochs` shipped **in this deployment**, so the
durable table it introduced was empty when the new process started. The registry
restored from nothing and began allocating again at 1. The live table now reads:

```text
paseo.agent|paseo-local|fe1eab37-…|1|2026-09-17T14:15:02Z
paseo.agent|paseo-local|0a2f4fde-…|2|2026-09-17T14:15:02Z
paseo.agent|paseo-local|3517f529-…|3|2026-09-17T14:15:02Z
paseo.agent|paseo-local|8a267de7-…|4|2026-09-17T14:15:02Z
paseo.agent|paseo-local|737758fc-…|5|2026-09-17T14:15:02Z
paseo.agent|paseo-local|d2f10bc9-…|6|2026-09-17T14:15:02Z
paseo.agent|paseo-local|6b7d0d08-…|7|2026-09-17T14:15:02Z
```

Seven mappings, numbered from 1, all recorded minutes after the 14:01 restart.
The numbering that issued epochs **12** and **13** is gone.

So `PaseoAdapter::wire_cursor(TimelinePosition { epoch: 12, .. })` finds no raw
spelling and returns `None`, and `history()` refuses with
`TimelineRefetchRequired { EpochChanged }` — **before any wire call**. That is
precisely why the plane looked healthy, why capabilities reported it reachable,
and why a direct read of the same session returned every event: nothing was ever
asked of Paseo.

## Why the response-anchored scan did not recover

`prove_current_turn` mapped *every* error from its history reads onto
`ProofScanIncomplete`:

```rust
.map_err(|error| { tracing::warn!(…); self.deny(ApiErrorCode::ProofScanIncomplete, …) })?
```

`TimelineRefetchRequired` is not "the read failed". It is the runtime naming a
disagreement about *which numbering the cursor is spelled in*, and it is the one
read error with a defined answer. Collapsing it lost two things at once: the
recovery was never attempted, and the operator was told to "settle again once
the session's canonical history is readable to its end" — advice that can never
come true, because the session was readable the whole time and the numbering is
never coming back.

The obvious recovery — re-read the session canonically from its origin — is the
unbounded read OG-061 exists to remove: Paseo's cursor-free path walks
`start_cursor` backwards until nothing older remains.

## The correction

Four seams, smallest at each boundary.

1. `RuntimeAdapter::refresh_timeline_epoch(binding)` — new trait method,
   defaulting to `Ok(())`. Asks the runtime which epoch the session is in now.
   Reads no content, so it costs one call at any session length.
2. `PaseoAdapter` implements it as a single `fetch_canonical(Tail, None, 1,
   Canonical)` plus `resolve_epoch(raw, None)`. One request, never a walk.
   Adapters stay store-free: what it allocates surfaces through the existing
   drain.
3. `ApiState::refresh_timeline_epoch_durably` — the same persist-before-expose
   barrier as `history_with_durable_epochs`, which now shares one
   `persist_drained_epochs` so no caller can take the drain without the write.
4. `Services::proof_scan_page` — every settlement history read goes through it.
   On `TimelineRefetchRequired` it refreshes the epoch **once**, then asks the
   same anchored question again. A second refusal is reported as
   `409 timeline_refetch_required` with the rule *"the claimed positions name a
   timeline epoch this runtime no longer maps, so this turn must be observed
   again before it can settle"* — not as a 503 inviting a retry that cannot
   succeed. `proof_scan_incomplete` keeps its original meaning: the scan ran out
   of reads.

No fence moved. Exact message identity at position, same epoch, terminality,
binding attestation, `waiting_input` and single-use are untouched, and every
outcome above writes nothing.

## Regressions

Both in `crates/kontor-daemon/tests/loopback_api.rs`.

- `a_settlement_answers_a_timeline_refetch_signal_once_and_settles` — the
  runtime refuses every anchored read until the epoch is re-read, then answers
  normally. The turn settles; exactly one refresh pays for the whole scan; the
  epoch the settled tuple is addressed by is durable; and the same tuple under a
  fresh key is still refused, with exactly one settled turn on the task.
- `a_settlement_naming_an_unmapped_epoch_is_told_to_observe_the_turn_again` —
  the live shape. A tuple true in every respect except the numbering it is
  spelled in, against a runtime that has forgotten which numbers it gave out.
  Refused `409 timeline_refetch_required`, one refresh not a loop, what the
  recovery allocated is durable before it returns, and nothing written.

## Mutation

| # | Mutation | Result |
|---|---|---|
| R1 | recovery guard matches `ReconciliationPending` instead of `TimelineRefetchRequired`, so the refetch signal returns straight through | **killed** by both tests |
| R2 | `unreadable_proof_scan` never takes its `TimelineRefetchRequired` branch | **killed** — reproduced the exact live 503 `proof_scan_incomplete` |
| R3 | `refresh_timeline_epoch_durably` returns without `persist_drained_epochs` | **survived first**; assertion was vacuous because an earlier ordinary read had already persisted the mapping. Test strengthened to forget the mappings first and assert the adapter holds nothing undrained after the refusal — the one path where the recovery's allocation is not carried by a later page. **killed** |

## What the preserved ASMA-8201/8202 tuples still need

`12:6 → 12:263` and `13:11 → 13:296` cannot be made to settle by any bounded
refetch, and should not be: those numbers name a dead allocation order, and
re-minting them would mean accepting a caller's epoch claim as its own evidence
— which is the single-use and same-epoch fence.

With this correction deployed, retrying them returns `409
timeline_refetch_required` instead of a misleading 503. The supported recovery
is one read-only `turn-observe` per run: it returns the same Kontor
`clientMessageId` with the positions in the *current* numbering, and that tuple
settles under the original idempotency key. No identity is replaced, no verifier
prompt is resent, and `kontor_turn_settle` remains the sole validator and writer.

That call is root's, after the next deployment.

## Guarded hotfix deployment plan

Same shape as the ASMA-8190 combined deployment, one restart.

1. Build release from the pushed head; record SHA256 for `kontor`,
   `kontor-daemon`, `kontor-mcp`.
2. `kontor-daemon --state-root <dir> snapshot`, then a labelled prune-proof copy
   `backups/kontor-pre-og-061-followup-<TS>.db`; record its hash and schema
   version.
3. Back up the three installed binaries to
   `deploy-backups/<TS>-og-061-followup-<sha>/`.
4. Stage and `rename(2)` each binary into place; write
   `deployments/ASMA-8203-<TS>-<sha>/{SOURCE_COMMIT,SHA256SUMS,candidate,previous}`.
5. One `launchctl kickstart -k gui/<uid>/com.asma.kontor.daemon`.
6. Read back: health `live`, schema version, `PRAGMA quick_check` and
   `integrity_check`, installed hashes against source, MCP worker tool count,
   and the census (projects, epics, tasks, runs, seats, settled turns).
7. Stop and report the safe checkpoint on any difference; do not improvise a
   rollback or a second restart.

No schema change ships with this correction — migration `0100` is already live
at schema 103 — so step 2's snapshot is precaution, not migration cover.
