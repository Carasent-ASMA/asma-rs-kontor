# KON-OP-06 code-review gate — checkpoint 4

Date: 2026-08-18
Ticket: ASMA-7875
Reviewed: `e58336d51d531ea77ad7d3aad6fb78a60572290e` (superproject pointer `451f55d2`)
Against: `docs/evidence/KON-OP-06/IMPLEMENT-HANDOFF.md`, `docs/evidence/KON-OP-06/ARCHITECTURE.md`

## Verdict — REJECTED

Rejected on one correctness defect introduced by this change (R1) and the
coverage gap that let it through (R2). Both are narrow and locally fixable; the
rest of the change is sound and nothing below asks for a redesign.

## Build verification at HEAD

Re-run independently in this worktree, not taken from the handoff:

| Check | Result |
| --- | --- |
| `cargo test --workspace` | exit 0 — 112 binaries, **1406 passed, 0 failed, 8 ignored** |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0, clean |
| `cargo fmt --all --check` | clean |

The 8 ignored are all environment-gated (`requires a live Paseo daemon`,
`requires two authenticated Codex accounts`); none is a completion proof. Every
test the handoff names by name exists and passes, including all four new
`repository_roundtrip.rs` cases, `completion_answers_from_its_own_repository_and_never_synthesizes`,
and the `every_uncomposed_successor_contract_refuses_rather_than_answering_emptily`
rename.

## Blocking findings

### R1 — `advance_completion` commits durable state on a path it then refuses, with no receipt

`crates/kontor-daemon/src/applications.rs:7909` starts the run when none exists,
and `start_completion` inserts the `epic_completion` row at
`crates/kontor-daemon/src/applications.rs:4988`. The `expected_revision` guard is
only reached afterwards, at line 7949, and the receipt is recorded later still,
at line 7973.

A first `POST …/completion:advance` whose `expected_revision` is anything other
than `AggregateRevision::INITIAL` (= 1, `crates/kontor-core/src/id.rs:903`)
therefore:

1. durably creates the epic's completion run, pinned to the built-in profile;
2. returns `409 revision_conflict`;
3. records no command receipt for the write it just performed.

Two things are wrong with that. The mutation escapes the receipt ledger, which
is the discipline the whole checkpoint is built on — every other write in this
change records before returning. And the refusal reason, *"the completion run
moved since the caller read it"*, is false: the caller could not have read it,
because `GET /epics/{id}/completion` answers `404` until this very call creates
the row. The caller is told about a race that did not happen.

The path is reachable by ordinary use. Because the `GET` is a `404` before the
first advance, a caller has no revision to read and must supply `1` from
knowledge of the type's initial value; any other guess starts a run it was
refused permission to start.

Suggested shape of the fix: derive the intent and judge the key, then guard the
revision against `AggregateRevision::INITIAL` when no row exists, and only then
call `start_completion` — or record the start under its own receipt before the
guard. Either keeps the ledger total.

### R2 — the two operations carrying the state machine have no API-level test

`grep` over `crates/` finds `completion:advance` and `completion:remediate` only
in route registration (`crates/kontor-api/src/lib.rs:467`, `:471`), the OpenAPI
annotations (`crates/kontor-api/src/applications.rs:5618`, `:5652`) and the MCP
registry (`crates/kontor-mcp/src/registry.rs:3363`, `:3387`). No test exercises
either route.

Of the six composed operations the handoff lists, four are covered by
`completion_answers_from_its_own_repository_and_never_synthesizes` — catalog,
preview, apply, and the `404` on an unstarted run — and the two that carry the
transition machine and the two-authority remediation rule are covered by none.

This matters specifically because of the defect the handoff reports fixing in
§"One defect found and fixed during implementation": the revision guard running
ahead of idempotency-replay recognition. That fix was applied to all three
writes, and the regression is pinned for `apply_completion_profile` only (the
publish → replay → stale-`expected_revision` sequence at
`crates/kontor-daemon/tests/loopback_api.rs:11753`). The ordering is in fact
correct in all three handlers — I read each one — but nothing holds it there,
and R1 is exactly the class of ordering mistake an `:advance` test would have
caught on its first assertion.

The store-level tests are good and do not close this: they prove the repository's
rules (immutable republication refused, superseded transition writes nothing,
replayed wake reuses its intent, second proposal per round refused), not the
handler composition above them.

## Verified as claimed

Checked and correct, recorded so the next gate need not redo them:

- **Idempotency before revision guard, all three writes.** `apply_completion_profile`
  (line 7819), `advance_completion` (7925) and both `remediate_completion`
  branches (8063 for the LSA proposal, 8156 for the TPM route — the
  `stale_revision` flag is computed early but only consulted inside
  `if !replayed`) each judge the key first. The comments explaining why are
  accurate.
- **Migration v32 is a clean rebuild.** Applying all 32 shipped migrations in
  order and diffing the resulting `command_receipts` DDL against v31 shows
  exactly the three added command kinds and nothing else — no column, constraint
  or index lost across the drop-and-rename.
- **The declared architecture deviation is sound.** `ProfilePreviewRequest`/
  `ProfileApplyRequest` are genuinely shared with `advisor-profiles` and
  `committee-templates`; retyping `definition` would have forced those to be
  completion specs. The strict decode instead happens in
  `compile_completion_definition`, and `deny_unknown_fields` on the two spec
  types (`crates/kontor-scheduler/src/completion.rs:21`, `:35`) means an
  unmodelled key is still refused before the definition is hashed. The
  requirement is met; accept the deviation.
- **The built-in is not shadowable.** `operational_default` is refused at
  publish, and is a read-path constant rather than a seeded row.
- **The refusals are honest.** `observe_completion` returns `Unavailable` with a
  named reason where OP-05's committee verdicts, integration TeamRun outcomes and
  closeout receipts are uncomposed, rather than synthesizing a pass. The ticket
  gate is derived from pinned work profiles and `artifact_evidence`, never from a
  task lifecycle value.

## Non-blocking observations

- **Evidence hygiene.** `docs/evidence/KON-MVP-18/run-011d346efca5905e/` records
  42 pass / 0 fail but is stamped `commit abb5c432`, the *parent* of the reviewed
  commit, so its ACCEPT does not cover any checkpoint-4 code. That directory is
  also untracked while all 18 sibling `run-*` directories are committed, and
  nothing gitignores it. Re-run against `e58336d` before citing it as evidence
  for this checkpoint.
- **Console types are stale.** Self-flagged in the handoff:
  `contract/openapi.json` was regenerated but
  `pnpm --filter kontor-console generate:api` was not run.
- **Pre-existing, not this change.** No triggers survive on `command_receipts`:
  `command_receipts_identity_immutable` and `command_receipts_no_delete`
  (`crates/kontor-store/migrations/0001_init.sql:1723`, `:1733`) are dropped by
  the table-rebuild pattern and never recreated. Verified by applying the
  migrations in order — they are already absent at `user_version = 23`, well
  before this commit. v32 inherits the pattern rather than introducing it, so it
  is not charged against this review, but the receipt ledger's append-only
  guarantee currently rests on application code alone.

## Note for the next seat

`git diff` and `git show --stat` return empty output in this worktree's sandbox
even across commits with differing tree hashes — confirmed against a known-good
pair. Anything "verified clean" with `git diff` here is a false negative. The
diffs behind this review were reconstructed with `git ls-tree -r` and
`git cat-file -p` piped through `diff`.

## To clear the gate

1. Fix R1 so no durable completion state is created on a refused advance, and so
   the refusal reason matches what actually happened.
2. Add API-level coverage for `completion:advance` and `completion:remediate`,
   including the replay → stale-`expected_revision` sequence for each, mirroring
   what `:apply` already has.
3. Re-run the KON-MVP-18 pilot harness against the fixed HEAD and commit the
   bundle.

Items 1 and 2 are the gate. Item 3 is required before the evidence is cited.
