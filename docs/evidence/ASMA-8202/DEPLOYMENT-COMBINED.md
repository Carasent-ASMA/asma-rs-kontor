# ASMA-8202 combined-head deployment and acceptance

Date: 2026-09-17

This record is additive. It does not amend [DEPLOYMENT.md](DEPLOYMENT.md),
[MUTATION.md](MUTATION.md) or [INTEGRATION.md](INTEGRATION.md), which remain
immutable point-in-time receipts. It supersedes `DEPLOYMENT.md` as the
description of what the realm now runs.

## Why a combined head

ASMA-8202 merged to `master` as `304ec707` (#228) and ASMA-8203 as
`86ba6065` (#229). The realm was running neither: the daemon installed at
`07:48:19Z` contained no `runtime_attachment_unconfirmed`, no
`turns/current` and no window check — zero hits on every short fragment. The
deployment owed was therefore the combined head, not either change alone.

Deployed source commit: `86ba6065494eabb5fbe56e1ec4d6b432f08779ed`.

## The two changes do not contend

ASMA-8203 changes `prove_current_turn` at approximately line 16408 of
`crates/kontor-daemon/src/applications.rs`. ASMA-8202 adds
`recover_unconfirmed_admissions` at approximately line 844 and alters the
ready-plan blocked projection at approximately line 25996. They share no region
and no state. ASMA-8203 also re-pinned the tool surface (worker profile 18→19,
observer-visible reads 10→11, registry 178→179); ASMA-8202 adds no route and no
MCP tool, so none of those pins move because of it.

Both features' own regressions pass on the one tree — see below.

## Qualification on `86ba6065`

| Gate | Result |
| --- | --- |
| `cargo test -p kontor-store --test scheduler_admission` | 32 passed, 0 failed |
| `a_partially_seated_candidate_claims_progress_and_an_unattached_one_does_not` | passed |
| `an_observed_turn_is_the_tuple_settlement_accepts_verbatim` | passed |
| `observing_a_current_turn_fails_closed_on_every_unreadable_shape` | passed |
| `observing_refuses_a_duplicate_that_straddles_the_resume_cursor` | passed |
| `observing_a_current_turn_pages_a_long_session_and_resumes_from_its_anchor` | passed |
| `settling_a_bounded_turn_refuses_a_forged_current_window` | passed |
| `a_failed_readback_after_delivery_never_reports_the_message_as_unsent` | passed |
| `kontor-tests-contract --test mcp_parity` | 12 passed |
| `kontor-tests-contract --test runtime_adapter` | 52 passed |

Both ASMA-8202 selector mutations were re-killed on the pre-merge integrated
head and are recorded in [INTEGRATION.md](INTEGRATION.md).

**Pre-existing on `master`, not introduced here.** `cargo clippy -D warnings`
fails at `crates/kontor-runtime/src/fake.rs:3618` — `Iterator::last` on a
`DoubleEndedIterator`. This was observed at pristine `origin/master` with a
clean working tree, so the attribution is definitive; the file's last-touching
commit is `86ba6065` itself. It is a lint, not a compile error, and the
release build is unaffected. It is left for ASMA-8203 rather than silently
patched here.

## The schema step is one-way

Migration `0099_turn_correlation_challenges.sql` moves the realm database from
schema 98 to 99. A pre-0099 daemon refuses a 99 database — the same class of
failure that took the realm down for roughly 32 seconds during an earlier
ASMA-8203 attempt. The deployment gate was therefore built so that **rollback
restores the database snapshot as well as the binaries**; restoring binaries
alone would have stranded the realm.

Snapshot taken before the step, and the rollback source:
`/Users/igor/.local/state/kontor/asma/deploy-backups/20260917T091805Z-asma-8202-combined-86ba6065/database/` — `integrity_check` `ok` at schema 98.

## Installed artifacts

| Artifact | SHA-256 |
| --- | --- |
| `kontor` | `e5b6efee18e86cf4591e5527d57c051b18ebda9b358df46f43886bc24d8d3e4e` |
| `kontor-daemon` | `d3c1f13484f5238d4bde30020bc22e29903cffc0f250a5b3e7dd534f3d0375c6` |
| `kontor-mcp` | `b53431d09126a9563f4704cc16ded90ead62fd47551d4b7f2e6554ae997c573d` |

Installed hashes were compared against the built hashes and matched exactly, so
the realm runs precisely this build and not a mixture. The replaced set is
retained at `/Users/igor/.local/state/kontor/asma/deploy-backups/20260917T091805Z-asma-8202-combined-86ba6065/old-binaries/`.

## Identity preservation

PID `8594` → `50145`. Schema `98` → `99`. `PRAGMA integrity_check`
returned `ok` and `PRAGMA foreign_key_check` returned zero rows.

The gate compared immutable identity before and after and passed byte-for-byte.
Volatile lifecycle was deliberately excluded from that comparison, because it
moves while the realm runs and its drift is not identity drift.

- realm `01a00649-9ee6-73e0-ba1b-6a6c35cfd065`
- project `01a0064a-e056-7603-9968-ef64fdaacb75`
- task `01a0ac9d-a966-7f03-9ead-2bd049094c7f`, still exactly one TeamRun
- TeamRun `01a0ad5a-7497-7e43-8c79-3a009be5238e`
- root AgentRun `01a0ad5a-7498-7170-9d63-e010e1ff2aad`
- root binding `01a0ad88-a307-7273-845d-ccf908e87c1b` on native
  `572d39a4-3a4b-46e4-8ec0-f0343be41f39`, generation 1
- the four seat ids and role slots `scope`, `implement`, `verify`, `audit`
- container `01a0ad5a-7e52-7550-a688-0f178208e897` on native workspace
  `wks_479843d94033d377` with its canonical worktree path

## Acceptance evidence

**Both surfaces are in the deployed daemon**, verified against the installed
binary rather than the build tree: `runtime_attachment_unconfirmed`,
`unconfirmed runtime attachments` (ASMA-8202), `lies inside the claimed
window` and `turns/current` (ASMA-8203), and `team_run_seat_fill` (#225).

**ASMA-8202.** The recovery selector matches zero rows on the live realm. That
is the correct steady state for a healthy fleet and is the no-false-positive
half of the claim: every admitted root is attached, so none is retried.

**ASMA-8203, authenticated against the running daemon.** An unauthenticated
probe was rejected as evidence: a nonexistent path also answered `401`,
proving only that authentication precedes routing. Authenticated reads settle
it:

- `kontor turn-observe --agent-run-id 01a0ad5a-7498-…` → **200**, returning
  `message_id 1d5f4507-c4a3-7ed5-8ab6-89a6bbcb9bec`, `message_sequence 1343`,
  `response_sequence 1368`, `timeline_epoch 2`, anchor
  `01a0ad88-a307-7273-845d-ccf908e87c1b:2:1368`. Repeating the read returned
  the identical tuple.
- a well-formed but unknown AgentRunId → **404 `not_found`**, so the handler is
  genuinely reached and refuses on its own rule.

The observation read answers **on the ASMA-8202 root run itself**, over the
binding ASMA-8202 preserved. That is the combined claim in one fact: admission
recovery kept the run's identity, and the observation surface reads that exact
identity without either change disturbing the other.

Live state after deployment: task `in_progress@2`, root run `confirmed`,
implement run `confirmed`, schema 99.

## Deployment history note

A deployment of the pre-merge ASMA-8202 head was performed earlier and reversed
because it overwrote the ASMA-8203 build then under verification; that episode
is recorded in [INTEGRATION.md](INTEGRATION.md). Backup for the present
deployment: `/Users/igor/.local/state/kontor/asma/deploy-backups/20260917T091805Z-asma-8202-combined-86ba6065`.
