# ASMA-8112 independent review notes

Date: 2026-09-06

Reviewed commit: `9854faad10ce191960801de9c2a2c0a096c2fd33`
("fix(kontor): Authorize a targetless never-bound handoff (ASMA-8112)") on
`fix/ASMA-8112-targetless-handoff`, parent `7ec7d0e`.

Remediation re-reviewed: `92eab949df25b64b470430f2b150a8173f3bc33f`
("test(kontor): Pin the mixed never-bound candidate set"). Both findings below
are closed; see "Remediation disposition". The verdict is unchanged. The full
`kontor-daemon` suite is green at `92eab94` — 393 passed, 0 failed, 1 ignored —
measured on a clean rebuild; see "Final verification".

Verdict: **PASS**. The repair is minimal, correctly fenced, and every invariant
this review was handed is satisfied. Two low-severity findings are recorded
below; neither is a production defect and neither blocks merge. Two open
questions are recorded for the ledger rather than resolved here, because the
diff and its evidence do not carry enough to settle them.

No production code was modified by this review. This commit adds only this
document.

## Scope

`git show --name-only 9854faa` touches exactly three files:

- `crates/kontor-daemon/src/applications.rs` (+25/-8, one arm of `replace_seat`)
- `crates/kontor-daemon/tests/loopback_api.rs` (+424, one hunk at 22401)
- `docs/evidence/ASMA-8112/IMPLEMENTATION.md` (+134)

No API, MCP, OpenAPI, schema or migration change, as claimed.
`turn_dispatches.target_agent_run` is already `NULL`-able
(`crates/kontor-store/migrations/0018_role_turns.sql:119`), so the state the
repair reads was always representable.

## Invariants checked

Each was read in the source at the reviewed commit, not inferred from the
implementation note.

| Invariant | Where enforced | Result |
| --- | --- | --- |
| Only an exact operator-abandoned never-bound predecessor | `applications.rs:25803` (`is_operator_abandoned_unbound`), re-proved at `26021` (`Abandoned` + `OperatorAbandon`) | Holds |
| Binding generation zero for a never-bound predecessor | `applications.rs:25735-25742` | Holds |
| Explicit Admin model route required | `applications.rs:25796`; re-required on the account-pin replay path at `26069` | Holds |
| Admin capability on the route | `kontor-api/src/applications.rs:10720` (`CallerCapability::Admin`) | Holds, unchanged |
| Same `TeamRun` and role slot | role equality `applications.rs:25744`; candidate filter `25813-25817` scopes to `predecessor.team_run_id` and `role_slot` | Holds |
| Exactly one undelivered candidate may be targetless | `applications.rs:25832-25835` | Holds |
| Zero candidates refuse | both disjuncts false → `25843` | Holds |
| Multiple candidates refuse | slice pattern `[only]` cannot match len ≠ 1 | Holds |
| One candidate naming another run refuses | guard `only.target_agent_run.is_none()` | Holds |
| Mixed `[targeted-elsewhere, targetless]` refuses | len 2 → `[only]` does not match; `pending_dispatch` false | Holds, but untested — see Finding 1 |
| Provider-outage / quota evidence excluded on this arm | `applications.rs:25791` | Holds |
| Predecessor and task revision fences | `25723`, `25749` | Holds |
| Terminal-team fence | `25772` | Holds |
| Team-definition-migration fence | `25767`, before command intent and any runtime call | Holds |
| Idempotent replay | `already_replaced` `25836-25842`; recorded-successor return `26050-26092` | Holds |
| `parent_agent_run_id` lineage | `reserve_after_unbound_abandonment` `kontor-teams/src/run.rs:1294` sets `parent: Some(abandoned)` | Holds |
| Original dispatch and message delivery preserved | `retry_undelivered_dispatches` `applications.rs:25067` re-resolves `seat_for_slot` and ignores the recorded target; `replace_seat` drives it in-band at `26302`; `mark_turn_dispatched` then writes the reached seat | Holds |

### The exact-target path is provably unchanged

The old gate was a single `.any()` over four conjuncts. The new code factors the
first three into `undelivered` and applies the fourth as `.any()` over that
vector. The predicate composition is identical, so `pending_dispatch` has the
same value for every input as before. The widening is confined to the added
`targetless_dispatch` disjunct.

### Why the widening cannot create a second live seat

`reserve_after_unbound_abandonment` (`kontor-teams/src/run.rs:1298-1304`)
refuses unless the slot's head has no live run *and* its lineage is empty, and
`slot_members` (`applications.rs:26094-26100`) excludes operator-abandoned
unbound rows from hydration. A targetless row that authorized the wrong
predecessor would still meet a vacant-slot requirement it cannot satisfy. This
is a real second fence, and it materially bounds the blast radius of Finding 1.

## Findings

### 1. The recorded interpretation of "exactly one" is not pinned by any test (low) — RESOLVED in `92eab94`

`IMPLEMENTATION.md` records a deliberate choice: the authorizing count is taken
over the *whole* candidate set, so `[targeted-at-another-run, targetless]`
refuses. The narrower reading it rejects — count only the targetless rows —
would authorize that mixed case.

No test distinguishes the two. Verified by mutation, not by argument alone:
replacing the gate with the narrower reading

```rust
let targetless_dispatch = undelivered
    .iter()
    .filter(|dispatch| dispatch.target_agent_run.is_none())
    .count()
    == 1;
```

leaves all seven `never_bound` tests green (`7 passed; 0 failed`). The two new
negatives do not separate the readings: two targetless rows refuse under both,
and one row naming another run has zero targetless rows and so refuses under
both. Inspection supports the same conclusion beyond the focused family — the
only fixture in the suite that writes `turn_dispatches.target_agent_run` is the
new test at `loopback_api.rs:22796`, and it re-points a single row.

Consequence: a future refactor to the narrower form would widen authorization
and pass the whole suite. That is a regression-pinning gap, not a live defect —
and the mixed state may not even be reachable, since a row naming a different
run in this slot implies a slot occupant that `reserve_after_unbound_abandonment`
would reject anyway. Suggested, not required: one test asserting the mixed set
refuses.

Scope of the mutation evidence: the focused `never_bound` family (7 tests). The
claim that no *other* test pins the interpretation rests on inspection of the
fixtures, not on a full-suite mutation run.

### 2. The validation line in `IMPLEMENTATION.md` overstates the result (low) — RESOLVED in `92eab94`

`IMPLEMENTATION.md` states `cargo test -p kontor-daemon` — "391 passed, 0
failed, 0 ignored". That is wrong in two ways: it counts the ignored test as
passed, and it omits a target.

The measured figure at the reviewed commit `9854faa` is:

```
392 passed; 0 failed; 1 ignored
```

**Correction to this review's first round.** The figure originally recorded here
was `390 passed`, which was also wrong. `cargo test -p kontor-daemon` runs a
`unittests src/main.rs` target of 2 tests in addition to `unittests src/lib.rs`,
and the first round's arithmetic omitted it. The builder then adopted `390` in
good faith. Both numbers are superseded by the measurements in "Final
verification" below.

`tests/loopback_api.rs` reports `282 passed; 0 failed; 1 ignored` against 283
test functions. The ignored one is
`a_configured_jira_boundary_distinguishes_historical_from_native_completion`
(`loopback_api.rs:8194`), carrying `#[ignore = "superseded by kontor-jira native
connector contract tests"]`. It is pre-existing, unrelated to ASMA-8112, and
outside the single hunk this commit adds.

The per-target table in the note is correct as a count of test *functions* for
the targets it lists, but it omits the `src/main.rs` unit target entirely. Worth
correcting because the evidence document is the durable receipt, and "0 ignored"
is specifically a claim that nothing was skipped.

### 3. The zero-candidate refusal has no direct test (informational)

Both existing and new tests settle an upstream turn before attempting the
reroute, so no test reaches the gate with an empty candidate set. The behaviour
is correct by construction and is unchanged by this commit — under the old gate
zero rows also refused — so this is noted, not charged against ASMA-8112.

## Open questions

Recorded here rather than answered, because neither can be evidenced from the
diff, the implementation note, or the surrounding code.

### Q1. What is the operator's route back for a fan-in slot?

If two upstream roles both hand to the same never-bound slot and the seat is
abandoned before both settle, the slot accumulates two targetless undelivered
rows and the recovery refuses permanently: there is no admission event, and now
no unambiguous dispatch either. This is the ASMA-8112 state with the row count
changed. `waive_role_slot` appears to be the only remaining move, and
`a_waiver_completes_a_team_whose_declared_slot_was_never_bound` shows a waiver
can close a never-bound slot — but nothing states that waiving is the *intended*
remedy here rather than an accepted dead end. The refusal is deliberate and I am
not disputing it; what is unrecorded is what an operator does next.

Attaches to: `docs/evidence/ASMA-8112/IMPLEMENTATION.md`, "The repair".
Pre-existing: yes — the same state was unrecoverable under the exact-target gate.

### Q2. Should `replace_seat` fence on a role-slot waiver?

`replace_seat` consults no waiver. `retry_undelivered_dispatches`
(`applications.rs:25084`) skips waived slots, so a slot waived *after* its
dispatch was derived keeps that row undelivered forever. Under the targetless
rule that permanently-pending row is a standing authorization to create a
successor into a slot that was explicitly declared absent — and the successor
could never receive the message, because retry will keep skipping it.

The exact-target path has the same shape, so ASMA-8112 does not introduce this;
it does enlarge the set of rows that can stand as such an authorization. I could
not determine from the code whether the absent waiver fence is intentional.

Attaches to: `crates/kontor-daemon/src/applications.rs`, `replace_seat`
never-bound arm.
Pre-existing: yes.

## Observations, not findings

- The command intent (`applications.rs:25938-25948`) records `unbound_recovery`
  but not *which* authority admitted the request. That is consistent with intent
  semantics — the intent must be a function of the request, or the idempotency
  key would become sensitive to store state — but it does mean the ledger cannot
  later distinguish an exact-target reroute from a targetless one.
- `a_handoff_naming_another_run_refuses_a_never_bound_replacement` forges its
  precondition with direct SQL. That state may not be naturally reachable, but
  writing it directly is the right way to pin the gate's literal condition, and
  direct-SQLite fixtures are an established pattern in this file (18 uses).
- `turn_dispatches` is keyed `PRIMARY KEY (settled_turn_id, to_role_slot_id)`,
  so more than one candidate row requires more than one settled turn — exactly
  what the two-row negative constructs. The ambiguity the gate refuses is real
  and reachable.
- The defect premise checks out at the source: `derive_follow_ups` sets
  `target_agent_run` from `seat_for_slot` (`applications.rs:3302`, `3414`),
  which filters `terminal.is_none()`, so an abandoned predecessor cannot be
  named.

## Independent validation performed

All runs are against the reviewed commit with a clean working tree, except the
two mutations, which ran in a detached throwaway worktree
(`/private/tmp/asma-8112-review-mutation`, removed afterwards) so that the
reviewed tree was never modified.

| Check | Result |
| --- | --- |
| `cargo test -p kontor-daemon` (full) | exit 0 — 392 passed, 0 failed, 1 ignored (see the correction in Finding 2; first recorded here as 390) |
| Focused `never_bound` (7 tests) on the reviewed build | 7 passed, 0 failed |
| Mutation A — `targetless_dispatch = false` (the unrepaired gate) | `an_admin_reroutes_a_never_bound_seat_whose_handoff_recorded_no_target` fails with exactly `409 revision_conflict` / "no pending handoff dispatch or recorded successor authorizes this never-bound seat"; other 6 pass |
| Mutation B — narrower "count only targetless rows" | all 7 pass — see Finding 1 |
| `cargo fmt -p kontor-daemon -- --check` | clean |
| `cargo clippy -p kontor-daemon --all-targets` | clean, no diagnostics |

Re-review of `92eab94`, same method:

| Check | Result |
| --- | --- |
| `crates/*/src` vs `9854faa` | byte-identical |
| Focused `never_bound` at `92eab94` (8 tests) | 8 passed, 0 failed; 284 enumerated in `loopback_api` |
| Mutation B repeated at `92eab94` | only `a_mixed_targetless_and_mistargeted_pair_...` fails, with `400 invalid_request` / subject `ModelRoute`; other 7 pass |
| Mixed test × 20 under concurrent load | 20 passed, 0 failed — no intermittent 400 |

### Final verification at `92eab94`

Measured after `cargo clean -p kontor-daemon` and a rebuild from the pristine
main worktree, with no mutation artifacts anywhere in the target directory:

```
393 passed; 0 failed; 1 ignored
```

| Target | Result |
| --- | --- |
| `unittests src/lib.rs` | 71 passed |
| `unittests src/main.rs` | 2 passed |
| `tests/account_pinning.rs` | 5 passed |
| `tests/loopback_api.rs` | 283 passed, 1 ignored (284 enumerated) |
| `tests/mcp_journey.rs` | 2 passed |
| `tests/quota_observation.rs` | 21 passed |
| `tests/recovery_security.rs` | 6 passed |
| `tests/succession_handoff.rs` | 3 passed |
| Doc-tests | 0 |

All four ASMA-8112 tests pass in that run, including
`a_mixed_targetless_and_mistargeted_pair_refuses_a_never_bound_replacement`.
The same totals and the same per-target breakdown were obtained independently
in a separate isolated target directory
(`CARGO_TARGET_DIR=/private/tmp/asma-8112-root-target`), agreeing exactly.

Mutation A independently confirms the implementation note's claim that the
positive test fails against the unrepaired gate with the exact production
refusal. That is the single most important thing to have verified here: the new
test genuinely discriminates the fix rather than passing for an unrelated
reason.

## Remediation disposition

Re-review of `21cd27e..92eab94` only. Verdict unchanged: **PASS**.

Production is byte-identical to the reviewed commit —
`git diff 9854faa 92eab94 -- 'crates/*/src'` is empty. The delta is one added
test and the corrected validation section.

**Finding 1 — closed.** `a_mixed_targetless_and_mistargeted_pair_refuses_a_never_bound_replacement`
builds `[names another run, names nothing]` and asserts both counts before the
call, so the narrower reading would see its single targetless row and authorize.
Re-running the narrowing mutation at `92eab94`: **only this test fails**, the
other seven pass. It is the discriminator, and it is the only one.

**Finding 2 — closed, with a further correction.** The note now names
`a_configured_jira_boundary_distinguishes_historical_from_native_completion` and
its pre-existing `#[ignore]`, and separates enumerated from run counts. But the
total it adopted, 390, came from this review's first round and was itself short
by 2: `cargo test -p kontor-daemon` also runs a `unittests src/main.rs` target
that both the original table and that first round omitted. The measured totals
are 392 at `9854faa` and 393 at `92eab94`, and this commit corrects both
documents to the figures under "Final verification". The error was mine, not the
builder's; they adopted a number this review supplied.

**The nonreproduced first-run 400 — evaluated, not a material risk.** The
mutation run explains it exactly. Under a wrongly-authorizing gate this test
does not fail on a created seat; it passes the gate and dies one step later at
`account_for_explicit_provider_alias`, because the negatives seed no
`codex-personal` recovery account. The observed failure is
`400 invalid_request`, subject `ModelRoute` — the precise status the builder
saw once. So a 400 here is the signature of *the gate having authorized*, and
must never be read as noise.

Three things close the risk. Production is unchanged from the commit already
mutation-tested and reviewed in full. The committed test ran 20/20 green under
concurrent load, with no intermittent 400. And the precondition assertions now
pin the candidate set immediately before the call, so the most plausible cause —
a degenerate set holding one targetless row, which the shipped gate would
correctly authorize — would now fail loudly at a named count assertion instead
of surfacing as an unexplained status. The new `omega_k3_seats == 1` assertion
is sound and worth keeping for the reason given, though it guards the separate
`already_replaced` authority rather than this.

One correction for the record: the comment "a gate that wrongly authorizes must
fail here on a created successor" is not what happens. Account resolution
refuses first, so no successor is created and the seat-count assertion stays
green — the status assertion is the whole discriminator. This is true of all
three negatives. It does not weaken them; the recorded rationale simply claims
more than the test proves.

### A false failure this review produced, and how it was cleared

Recorded because the ledger should show why an intermediate "failure" was
reported and then withdrawn.

Both mutation experiments were built with `CARGO_TARGET_DIR` pointed at the main
worktree's target directory, to reuse its warm dependency cache. Cargo derived
the *same* output filenames for the mutated crate as for the pristine one — the
test binary was `loopback_api-b5159e50cf44b3f2` in both trees — so the mutated
`kontor-daemon` artifacts overwrote the pristine ones and were not fully
replaced by the subsequent ordinary rebuild.

Everything built in that directory afterwards inherited the narrowing mutation.
Two full-suite runs and a 72-run concurrency stress therefore reported
`a_mixed_targetless_and_mistargeted_pair_refuses_a_never_bound_replacement`
failing with `400 invalid_request` / subject `ModelRoute` — 72 out of 72, which
is precisely the mutation's signature and not a flake. The tell was that
determinism: the same test had passed 20/20 minutes earlier on a binary
preserved from before the first mutation build.

`cargo clean -p kontor-daemon` followed by a rebuild from the pristine worktree
cleared it immediately — 8/8 in the focused family, then the full green run
under "Final verification", confirmed again in a wholly separate target
directory. Production source was never modified: `git status` stayed clean
apart from this document throughout.

Two conclusions. The failure was a reviewer methodology error and says nothing
about the code. And the builder's nonreproduced first-run 400 remains
*unexplained by any evidence in the record* — it was never reproduced on a clean
build here, across the full suite, the focused family and 20 isolated repeats.
It is not a standing risk, and the test's precondition assertions would now
catch the degenerate candidate set that is its most plausible cause, but this
review cannot claim to have found its cause.

## Recommendation

Merge. Both findings are closed in `92eab94` and no new blocking finding was
raised. Q1 and Q2 remain open in the ledger and should not hold this change.
Optional, non-blocking: correct the "fails on a created successor" comment on
the three negatives to name the status assertion as the discriminator.
