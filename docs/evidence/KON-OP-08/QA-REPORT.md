VERDICT: PASS

# KON-OP-08 / ASMA-7877 — QA gate report

Tester: qa-gate seat (relaunch; the first instance was killed by a Paseo restart
before doing any work, so nothing here is inherited from it).

This is **QA, not code review**. The independent post-hoc code review
(`KON-OP-08-REVIEW-NOTES.md`, verdict PASS, 7 findings: 1×P2 + 6×P3) was read
first and is not redone. What follows is an independent test run, a behavioural
check of the staged-hop fix, a coverage judgement on each of the inspector's
seven findings, and a contract-artifact sync check.

## Scope and pinning

| Item | Value |
| --- | --- |
| Pinned SHA | `527cc164e822d0cd9cc03aa8c53d2f532a80cea9` (PR #64 merge commit) |
| Worktree | `~/.cache/op08-qa/tree` — private, detached, created with `git worktree add --detach` |
| Toolchain | `1.97.1-aarch64-apple-darwin` (workspace `rust-version = "1.97.1"`) |
| Tree state | `git status --porcelain` clean apart from one test-generated artifact (see §6) |
| Logs | `~/.cache/op08-qa/logs/`, exit codes appended live to `~/.cache/op08-qa/EXITCODES.txt` |

The shared checkout `_tools/asma-rs-kontor` was **not** written to, committed to,
staged, stashed, reset, or branch-switched. It sat on
`feat/ASMA-7869-command-execution-mode` at `16ab607` when I started. The only
things I read from it were its `node_modules` (for the console check, §5) and its
git object database (as the worktree's backing store).

Every command below was run bare with `$?` captured immediately — no pipes into
`grep`/`head`, so no exit code is a pipeline's last stage.

## 1. Full workspace suite — per package

All 22 workspace members, not just the 10 the change touches. Runner:
`~/.cache/op08-qa/run-suite.sh`, one `cargo test -p <crate>` per package,
result appended to `EXITCODES.txt` as each finished.

| Command | Exit | Binaries | Passed | Failed | Ignored | Wall |
| --- | --- | --- | --- | --- | --- | --- |
| `cargo test -p kontor-core` | **0** | 8 | 178 | 0 | 0 | 21s |
| `cargo test -p kontor-store` | **0** | 21 | 325 | 0 | 0 | 450s |
| `cargo test -p kontor-runtime` | **0** | 3 | 51 | 0 | 0 | 12s |
| `cargo test -p kontor-context` | **0** | 5 | 33 | 0 | 0 | 9s |
| `cargo test -p kontor-accounts` | **0** | 3 | 33 | 0 | 0 | 27s |
| `cargo test -p kontor-teams` | **0** | 4 | 47 | 0 | 0 | 4s |
| `cargo test -p kontor-scheduler` | **0** | 5 | 47 | 0 | 0 | 5s |
| `cargo test -p kontor-calendar` | **0** | 4 | 47 | 0 | 0 | 4s |
| `cargo test -p kontor-policy` | **0** | 5 | 39 | 0 | 0 | 3s |
| `cargo test -p kontor-profiles` | **0** | 5 | 39 | 0 | 0 | 10s |
| `cargo test -p kontor-intake` | **0** | 7 | 26 | 0 | 0 | 3s |
| `cargo test -p kontor-integrations-asma` | **0** | 3 | 35 | 0 | 0 | 16s |
| `cargo test -p kontor-runtime-paseo` | **0** | 4 | 191 | 0 | 6 | 15s |
| `cargo test -p kontor-runtime-ao` | **0** | 4 | 78 | 0 | 1 | 10s |
| `cargo test -p kontor-runtime-codex` | **0** | 4 | 37 | 0 | 1 | 4s |
| `cargo test -p kontor-api` | **0** | 4 | 28 | 0 | 0 | 30s |
| `cargo test -p kontor-mcp` | **0** | 4 | 51 | 0 | 0 | 16s |
| `cargo test -p kontor-cli` | **0** | 3 | 16 | 0 | 0 | 33s |
| `cargo test -p kontor-daemon` | **0** | 6 | 231 | 0 | 0 | 266s |
| `cargo test -p kontor-tests-contract` | **0** | 9 | 104 | 0 | 0 | 51s |
| `cargo test -p kontor-tests-e2e` | **0** | 4 | 2 | 0 | 0 | 42s |
| `cargo test -p kontor-desktop` | **0** | 3 | 0 | 0 | 0 | 53s |
| **TOTAL** | **all 0** | **118** | **1638** | **0** | **8** | ~18m |

`grep -l "test result: FAILED"` over all 22 logs → no match (grep exit 1).
No package needed a retry; **no exit 143 occurred**, so no SIGTERM/lock
contention affected this run.

The 12 packages beyond the reviewer's 10 all pass too, so nothing downstream of
the change regressed either.

### The 8 ignored tests — all opt-in live-harness probes, none silently skipped logic

Named here because "8 ignored" in a QA report should never be left unexplained:

- `kontor-runtime-ao`: `live_smoke_launches_two_clients_concurrently` — requires a disposable AO daemon.
- `kontor-runtime-codex`: `two_pinned_accounts_run_concurrently_and_neither_ending_closes_a_run` — requires two authenticated Codex accounts + a disposable worktree.
- `kontor-runtime-paseo` (6): `live_a_correlated_session_read_round_trips`, `live_adopted_root_places_and_archives_a_child_and_registers_no_project`, `live_an_unknown_agent_is_refused_rather_than_answered_empty`, `live_cli_reports_the_supported_baseline`, `live_hello_is_accepted_and_the_daemon_pushes_a_pinned_identity`, `live_the_status_readback_agrees_with_the_pushed_identity` — all require a live Paseo daemon.

Each requires external infrastructure this gate does not have. **I could not run
these**, and they are not evidence for or against this change. None of them
covers any of the seven findings.

### Lint and format

| Command | Exit | Result |
| --- | --- | --- |
| `cargo clippy --all-targets --workspace` | **0** | zero warnings, zero errors emitted |
| `cargo fmt --all --check` | **0** | no diff |

## 2. The Jira staged-hop defect — behavioural verification

The defect this ticket fixed: a request that **declared one destination while
carrying the route to another**. `build_write_request` sent
`destination: plan.target` (the milestone) while `transition` carried the hop —
so a hop that succeeded produced a receipt that read as if the milestone had been
reached.

I did not take the existing test's word for it. I wrote a throwaway probe
(`qa_probe_hop_and_direct_are_consistent_and_hash_apart`) that drives **both**
legs of a real staged sequence through the fake connector and asserts on the JSON
actually written to its stdin.

```
cargo test -p kontor-integrations-asma --test contract qa_probe -- --nocapture
=> EXIT=0   (1 passed, 0 failed, 35 filtered out)
```

The probe was appended to my private worktree's `contract.rs`, run, and the file
restored byte-for-byte from a pre-probe copy. It never existed in the shared
checkout and is not part of the change. Kept at `~/.cache/op08-qa/qa-probe.diff`.

### Result — internally consistent

| Attempt | Standing | `destination.status_id` | `transition.to_status_id` | Consistent |
| --- | --- | --- | --- | --- |
| 1 — staged hop | third inbound status | hop (`10213`) | hop (`10213`) | ✅ |
| 2 — direct move | hop (`10213`) | milestone | milestone | ✅ |

Both documents agree with themselves, and the hop **never** claims the milestone.
Confirmed: `plan.destination()` is what crosses the boundary.

### Result — different intent digests

```
QA-PROBE hop_intent   = eb1433757815a19c234a6f1a679ac8145981a3f5a4d35dcd44d1b6579da6fb88
QA-PROBE direct_intent= d9c5b0bec412aa1d7451fd669e10a416d155331a92911f6ff7052a64e69e1fc1
```

Different. The hop and the direct move that follows it are **not** one replayable
command, so the second attempt cannot be mistaken for a retry of the first and
answered with the first one's receipt. This is the property that matters, and it
holds.

### QA-1 — the digest separates by accident of the observation, not by the plan (informational, not blocking)

Worth recording because the reviewer's reasoning for it is right about the
outcome but understates *why* it holds, and the distinction is load-bearing if
anyone edits this later.

`intent()` (`jira.rs:1017`) still sets `destination: &plan.target`, and the
selected transition is **not in the intent document at all**. So the digest is
blind to hop-vs-direct. My probe proves this directly — same observation, two
different plans (one hopping, one direct) to the same milestone:

```
QA-PROBE same_observation_hop_vs_direct_digests_equal=true
```

The two real attempts differ only because `prior_status_id`,
`prior_observation_hash` and `live_routes` all move when the ticket actually
lands on the hop.

Why this is **not** a defect today: `reconcile` is pure and total, so one
`(spec, observation, facts)` yields exactly one plan — the two colliding plans
cannot both be produced from one observation through any real path. And the
intent document never crosses the boundary; only its hash does
(`intent_hash: Some(intent.hash().clone())`), so no connector ever sees the
`destination` field disagree with the request's. The collision is unreachable and
unobservable.

Why it is still worth a line: the separation is a consequence of the observation
changing underneath the digest, not a property the digest asserts. Nothing fails
if a future change makes two distinct commands share one observation. A one-line
addition of `plan.destination()` (or `is_staged_hop`) to `DelegationIntent` would
make the guarantee intrinsic. Follow-up, not a blocker.

## 3. The seven findings — runtime observability and test coverage

For each: is it visible from outside the process, and would the suite go red if
it regressed or worsened? Verified against the pinned tree, not inferred from the
review.

| # | Sev | Runtime-observable? | Covering test? |
| --- | --- | --- | --- |
| F1 | P2 | **Yes** — `409 placement_blocked` | ❌ **none** |
| F2 | P3 | No, not with the shipped spec | ❌ **none** |
| F3 | P3 | **Yes**, with ≥2 seats | ⚠️ adjacent only |
| F4 | P3 | **Yes** | ❌ **none** |
| F5 | P3 | Only after a backward clock step | ❌ **none** |
| F6 | P3 | **Yes** — one GET | ❌ **none** |
| F7 | P3 | **Yes** — two POSTs | ❌ **none** |

**All seven are uncovered.** F3 is the only one with anything adjacent. Detail:

**F1 — per-epic topology upgrade blocks placement (P2, pre-existing, widened here).**
Observable: yes — after an authorized, zero-effect per-epic upgrade, every task
under that epic refuses with `409 placement_blocked`; the reviewer reproduced it
(probe exit 101) and proved it identical at `527cc16^1`.
Coverage: **none.** The one test that upgrades an epic pin —
`an_epic_pin_moves_only_through_the_preview_that_was_authorized`
(`loopback_api.rs:18254`) — materializes *before* the upgrade, then after
`upgrade:apply` only retitles, replays, and asserts two refusals (an invented
revision, a stale one). I enumerated every call in its body: **nothing is placed
after the successful apply**, which is exactly the step that would fail. This is
the most consequential gap of the seven.

**F2 — `staged_hop` does not require the hop be inbound-compatible (P3, new).**
Observable: **not today.** I checked the shipped fixture directly:
`reopen = 10213`, `inbound_compatible = [10237, 10213, 10214, 10229, 10234, 10254]`
→ the reopen selector **is** inbound-compatible, so the trap is latent.
Coverage: **none.** Neither `staged_hop` (`ticket.rs:1136`) nor
`ExternalWorkflowSpec::validate` (`ticket.rs:868`) enforces the invariant, and no
test asserts it. `both_external_workflow_fixtures_persist_reopen_and_hash_identically`
(kontor-store) pins the fixture *bytes*, which is not the same guarantee — it
would not fail if a new spec violated the invariant.

**F3 — TeamRun lifecycle is last-child-writer-wins (P3, new).**
Observable: yes, but only for a team with ≥2 seats in different states; the
TeamRun reports whichever child was observed last.
Coverage: **adjacent only.** `a_message_resume_reduces_the_run_and_team_run_back_to_running`
(`loopback_api.rs:1192`) drives one seat to `waiting_input` and asserts
`waiting_team.lifecycle == "waiting_input"` — that pins the *current* semantics
for a single seat, so switching to an aggregate would go red. But the whole
loopback suite contains only three `observed_state` literals, and **no test ever
puts two sibling seats in divergent states**, so the flapping itself is untested.
A deliberate decision (aggregate vs. latest) should come with the missing test.

**F4 — doc says "fresh", code never checks freshness (P3, new).**
Observable: yes — a stale observation still advances `lifecycle`. Confirmed in
source: `reduce_observation` holds `freshness` and passes it to `derive_run_state`
(`append.rs:241`) but calls `reduce_run_lifecycle(run.projection.lifecycle, observed)`
(`append.rs:257`) without it, so `derived` respects staleness and `lifecycle`
does not.
Coverage: **none.** Every reduction test I found supplies `Freshness::Fresh`.
Nothing exercises the stale path.

**F5 — wall-clock microseconds as the monotonic reduction key (P3, new).**
Observable: only after a backward clock step (NTP correction), and then
*silently* — `may_reduce` (`state.rs:1744`) requires strictly increasing
sequences, so the event is appended, the projection is not advanced, and nothing
errors.
Coverage: **none.** `timestamp_control_sequence` has **zero** references in any
test file across the whole repo — only the definition (`observation.rs:37`) and
six adapter call sites. Worth repeating the reviewer's framing: this is still a
clear improvement over the previous constant `native_sequence: 0`, which froze AO
and Codex projections after their first observation.

**F6 — `topology_inspect?epic_id=` now 404s for an unpinned epic (P3, new).**
Observable: yes, trivially — one GET against an existing, never-placed epic.
Coverage: **none.** The refusal string `"this epic is not pinned to a topology
revision yet"` appears only at `applications.rs:2892` and `:9263`, and **never in
a test file**. Neither the old fallback behaviour nor the new refusal was ever
pinned, which is why an unannounced read-surface change could land silently.

**F7 — non-terminal `settle_runtime` reports `applied: created` on a replay (P3, new).**
Observable: yes — POST `runtime:settle` twice with the same idempotency key
against a non-terminal run: the same `receipt_id` comes back both times while
`applied` says `created` both times. Confirmed in source: replay-ness is decided
at `applications.rs:16176` (`if let Some(existing) = self.replayed(...)`) but only
`existing.id` is kept, so the non-terminal branch at `:16296` cannot tell a
replay from a first call and returns `AppliedDto::Created` unconditionally. The
already-terminal branch at `:16206` correctly returns `Unchanged`.
Coverage: **none.** I checked all eight `runtime:settle` idempotency keys in
`loopback_api.rs` — every one is unique. **No test replays a non-terminal settle**,
so nothing asserts `applied` on the second call.

### What this means

A green suite on unreviewed code is a real result and it is reported as one. But
the seven findings are exactly the seven places where the suite would stay green
through a regression. That asymmetry is the honest headline of this gate: the
1638 green tests say the delivered behaviour works, not that these seven
behaviours are protected. None of it changes the verdict — the inspector judged
all seven non-blocking and I found nothing to contradict that — but F1 in
particular deserves a follow-up ticket with a test, since it is a live 409 on a
real operator path.

## 4. Contract artifacts — OpenAPI

In sync, proven by the crate's own pin rather than by inspection.

```
cargo test -p kontor-api  =>  EXIT=0
  tests/openapi_contract.rs — 3 passed, 0 failed:
    the_committed_contract_document_is_the_one_this_crate_serves      ok
    the_contract_document_names_every_route_the_router_exposes        ok
    the_committed_session_vocabulary_is_the_one_this_crate_subscribes_to  ok
```

`openapi_contract.rs` asserts the committed `crates/kontor-api/contract/openapi.json`
is byte-identical to `kontor_api::openapi::document()` as rendered, and pins
`apps/console/src/test/session-kinds.json` the same way. Both pass, so the
committed document *is* what this build serves.

Consistent with the diff: `git diff --name-only 527cc16^1 527cc16` does not touch
`openapi.json`, and it should not have — the change adds a trait method
(`persist_session_observation`) and post-send behaviour in `sessions.rs`, but no
route and no DTO field.

## 5. Contract artifacts — console types

Not covered by any Rust test (the pin stops at `openapi.json` and
`session-kinds.json`), so I verified it directly.

```
openapi-typescript <pinned openapi.json> -o /tmp/op08-schema-regen.d.ts   => EXIT=0
diff -u apps/console/src/api/schema.d.ts /tmp/op08-schema-regen.d.ts      => EXIT=0   (0 lines)
apps/console: tsc --noEmit                                                => EXIT=0
```

- `schema.d.ts` is **byte-identical** to a fresh regeneration from the pinned
  document — the committed types are the ones this contract produces.
- `types.ts` is hand-written aliases into `schema.d.ts`; `tsc --noEmit` passing
  proves every alias still resolves, so no alias points at a schema member the
  realm stopped serving.

Generator provenance: run from the shared checkout's `node_modules`
(`openapi-typescript` 7.13.0). Safe because I first confirmed
`pnpm-lock.yaml`, `package.json`, `pnpm-workspace.yaml` and
`apps/console/package.json` are **sha256-identical** between the shared checkout
and my pinned tree, so that install is a valid install for this commit. The two
`node_modules` symlinks I created in my worktree were removed afterwards.

## 6. Test-suite side effect on the working tree (pre-existing, not OP-08)

Running the suite is **not** side-effect-free: `tests/e2e/pilot.rs` writes a fresh
evidence bundle into the repository at
`docs/evidence/KON-MVP-18/run-<hash>/` (my run produced
`run-82c513de2bf5cef0`, manifest stamped `"commit": "527cc16…"`).

This is why my `git status --porcelain` shows one untracked directory rather than
nothing, and I am declaring it rather than quietly excluding it. It is
pre-existing and unrelated to this change, but two facts make it worth a line:
903 such files are already committed (5.7 MB under `docs/evidence/KON-MVP-18/`),
and any CI job that checks for a clean tree after `cargo test` will fail on it.
Not a finding against OP-08; a repo-hygiene follow-up for whoever owns KON-MVP-18.

## 7. What I could not run, and why

- **The 8 `#[ignore]`d live-harness tests** (§1) — need a live Paseo daemon, a
  disposable AO daemon, or two authenticated Codex accounts. None available to
  this gate. None of them covers any of the seven findings.
- **The console's own suites** — `vitest` and `playwright` were not run. Out of
  the brief's scope (item 4 is artifact sync, which §5 establishes) and the
  Playwright suite needs browsers this gate has no reason to install.
- **A live Jira boundary** — the staged-hop verification (§2) is against the fake
  connector, which is the correct level: the defect was an internally
  inconsistent *document*, and the document is what the probe asserts on. No real
  Jira instance was touched.
- **I did not re-run the reviewer's F1 reproduction probe.** It is already
  attested at both `527cc16` and `527cc16^1` in the review notes, and re-running
  it would re-do code review rather than QA.

## Verdict

**PASS.**

- 22/22 packages exit 0. 1638 tests passed, 0 failed, 8 ignored (all opt-in live
  harnesses), across 118 binaries, at the merge commit in a clean pinned tree.
- `cargo clippy --all-targets --workspace` exit 0 with zero warnings;
  `cargo fmt --all --check` exit 0.
- The staged-hop fix does what the ticket claims: a hop and a direct move each
  produce an internally consistent request, and the two attempts of a real staged
  sequence carry different intent digests. Verified on the wire, not on the plan.
- OpenAPI document and console types are both in sync with what is committed.

**Coverage gaps — reported as a result in their own right.** All seven inspector
findings are uncovered by the suite; five of them (F1, F3, F4, F6, F7) are
observable at runtime through ordinary API calls. F1 (P2) is the one to ticket
first: it is a live `409 placement_blocked` on a real operator path, and the only
test that touches an epic pin upgrade stops short of the step that would fail.
F3 additionally wants a deliberate aggregate-vs-latest decision recorded with a
sibling-divergence test. Informationally, QA-1: the intent digest is blind to
hop-vs-direct and separates the two attempts only because the observation moves
underneath it — unreachable today, cheap to make intrinsic.

None of this blocks. A green suite on unreviewed code is a real result; so is the
fact that these seven behaviours would regress silently.

I have recorded **no Kontor gate**. The LSA records it citing this verdict.
