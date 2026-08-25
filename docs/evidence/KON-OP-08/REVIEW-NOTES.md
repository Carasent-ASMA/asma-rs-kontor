VERDICT: PASS

# KON-OP-08 / ASMA-7877 — independent post-hoc review

Reviewer: independent inspector seat (replacement for the seat lost in the
2026-08-21 Codex outage). Review is post-hoc: the change was merged to
`origin/master` as PR #64 without code review.

## Scope reviewed

| Item | Value |
| --- | --- |
| Merge commit | `527cc164e822d0cd9cc03aa8c53d2f532a80cea9` |
| Diff reviewed | `git diff 527cc16^1 527cc16` — 27 files, +4324/-201 |
| Base (first parent) | `367a711` (PR #69) |
| Branch tip (second parent) | `d310cdc` |
| Commits reviewed | `git log 527cc16^1..527cc16^2` — 17 commits |

`527cc16` is an ancestor of the shared checkout's current HEAD, so the SHA-pinned
diff is the merged OP-08 change regardless of which branch that checkout sits on
(it moved twice during this review; it was on
`fix/ASMA-7869-serve-api-during-startup-reconciliation` at `4c6ebd5` when I
finished).

The `docs/evidence/KON-OP-08/*` files were read for intent only. Every claim below
is stated against code or against a test run.

## Test evidence

Primary attestation ran in a private detached worktree pinned to the merge
commit — **not** in the shared checkout, so no other session's work is mixed in:

```
tree:   ~/.cache/op08-review/tree
HEAD:   527cc164e822d0cd9cc03aa8c53d2f532a80cea9
status: git status --porcelain → empty (clean)
```

Commands, one per package, real `$?` captured per run into
`~/.cache/op08-review/exit-codes.txt`; no piped `grep` anywhere in the harness.

| Command | Exit | Result |
| --- | --- | --- |
| `cargo test -p kontor-core` | 0 | 178 passed, 0 failed / 8 binaries |
| `cargo test -p kontor-integrations-asma` | 0 | 35 passed, 0 failed / 3 binaries |
| `cargo test -p kontor-runtime` | 0 | 51 passed, 0 failed / 3 binaries |
| `cargo test -p kontor-store` | 0 | 325 passed, 0 failed / 21 binaries |
| `cargo test -p kontor-runtime-paseo` | 0 | 191 passed, 0 failed / 4 binaries |
| `cargo test -p kontor-runtime-ao` | 0 | 78 passed, 0 failed / 4 binaries |
| `cargo test -p kontor-runtime-codex` | 0 | 37 passed, 0 failed / 4 binaries |
| `cargo test -p kontor-mcp` | 0 | 51 passed, 0 failed / 4 binaries |
| `cargo test -p kontor-api` | 0 | 28 passed, 0 failed / 4 binaries |
| `cargo test -p kontor-daemon` | 0 | 231 passed, 0 failed / 6 binaries |

**All ten packages exit 0. 1205 tests passed, 0 failed, across 61 binaries.**
`grep -l "test result: FAILED"` over every log → no matches.

### Probe runs (deliberately failing, throwaway — never committed)

One review probe test was written to characterise F1. It is **not** part of the
delivered change. It was appended to `loopback_api.rs` in the shared checkout for
the first run and has been removed from there; that file is back to its exact
pre-probe bytes and `git status --porcelain` in the shared checkout is clean of it.

| Tree | HEAD | Probe exit | Outcome |
| --- | --- | --- | --- |
| shared checkout (at the time) | `1a9354e` | 101 | `409 placement_blocked` |
| `~/.cache/op08-review/base-527cc16` | `367a711` (= `527cc16^1`) | 101 | `409 placement_blocked` — identical |

The base failing identically is what demotes F1 from "regression this PR
introduced" to "pre-existing defect this PR widens". See F1.

## Prior findings — closed / not closed

### (a) `build_write_request` sent `destination: plan.target` while carrying a hop's transition — **CLOSED**

Three sites now ask `plan.destination()` instead of `plan.target`:

- `crates/kontor-integrations-asma/src/jira.rs:1170` — the `destination` field of
  the request that crosses the connector boundary. This was the defect.
- `crates/kontor-integrations-asma/src/jira.rs:1195` — `prove_live_route`, the
  fail-closed guard that the selected route still reaches the planned status.
- `crates/kontor-integrations-asma/src/jira.rs:973` — `reconcile_after_ambiguity`,
  so a hop that landed as planned is not judged `Contradictory` and retried.

`TransitionPlan::destination()` / `is_staged_hop()` are defined at
`crates/kontor-core/src/ticket.rs:1013` and `:1024`.

Covered by a **wire-level** test, not a unit assertion on the plan:
`crates/kontor-integrations-asma/tests/contract.rs:2039`
(`a_staged_hop_request_declares_the_hop_not_the_milestone`) drives
`delegation.dry_run` through the fake connector and asserts on the JSON actually
written to its stdin — that `destination.status_id` is the hop, that
`transition.to_status_id` agrees with it, and that it is *not* the milestone. That
is the right level: the defect was an internally inconsistent document, and the
document is what is asserted. Ran green (`kontor-integrations-asma`, exit 0).

One deliberate non-change worth recording so it is not mistaken for a miss:
`intent()` at `jira.rs:1017` still uses `&plan.target`. That is correct — the
intent digest is the replay key for "which milestone am I converging to", and the
hop attempt and the final attempt are already distinguished inside that document
by `prior_status_id` and `live_routes`, so two attempts at one milestone cannot
collide into a false replay.

Note the author's own `CHECKPOINT-01.md` lists only *two* corrected applier sites
and never mentions `build_write_request`. That is because the third fix landed
later, in commit `888bec6` ("fix(jira): Declare the hop a request performs, not
the milestone"), and the checkpoint doc was not updated. The code and the test are
what matter, and both are present.

### (b) The migration runner's hardcoded lineage list — **NOT CLOSED, and out of scope for this change**

`crates/kontor-store/src/migrations.rs` is **not in this diff**:
`git diff --name-only 527cc16^1 527cc16 | grep -c migrations.rs` → `0`. The only
commit touching that path in `527cc16^1..527cc16^2` is the merge `d783ccd` pulling
master in. The list is master's code, unchanged by OP-08.

The hazard as it actually stands at `migrations.rs:369-386`: the
`operational_hardening_lineage` branch replays a hand-written index list
(`MIGRATIONS[33,34,35,37..=46]`, deliberately skipping index 36). The compile-time
guard at `:212` (`MIGRATIONS.len() == SCHEMA_VERSION`) does **not** cover this
list.

The severity distinction matters, so stating it rather than leaving a scare:

- A **truncated tail** (schema 48 added, list not extended) fails **loudly**: each
  script sets its own `PRAGMA user_version`, so the sweep lands on 47 and
  `verify_applied` (`migrations.rs:742`, called from `migrate` at `:322`) refuses
  when `user_version != SCHEMA_VERSION`.
- A **skipped middle index** is **silent**: `user_version` still reaches
  `SCHEMA_VERSION` while that migration's objects were never installed.

Today the list does reach index 46 = migration 47 = `SCHEMA_VERSION`, so it is
correct as merged. P3, pre-existing, follow-up ticket.

### (c) `Services::jira_specs` selecting the first bundled pair rather than an auditable pin — **NOT CLOSED, and untouched by this change**

`crates/kontor-daemon/src/applications.rs:1590`. Still seeds from
`catalog.field_specs().first()` and `catalog.workflow_specs().first()`. The only
edit in that region of the diff is `refuse_asma`, not `jira_specs`.

It is harmless as shipped, and this is worth stating precisely: `SpecCatalog::bundled()`
loads exactly two `include_str!` constants — `jira.rs:59`
(`fixtures/ticket-fields-asma.json`) and `jira.rs:62`
(`fixtures/external-workflow-asma.json`) — one field spec and one workflow spec.
`.first()` is therefore deterministic and unambiguous today. It becomes a real
auditability hole the moment a second connector/project/issue-type pair is
bundled, because the selection would silently depend on load order rather than on
a pin. P3, pre-existing.

### (d) `Services::live_seat` including parked/abandoned terminal runs — **CLOSED**

`crates/kontor-daemon/src/applications.rs:2249`. Now skips terminal *team* runs
(`if lifecycle.is_terminal() { continue }`) and, per seat, loads the agent run and
requires `!run.projection.lifecycle.is_terminal()`.

Both states named in the finding are genuinely covered — this is the part worth
verifying rather than assuming, since neither word appears in `live_seat`:
`TerminalOutcome::Parked | Abandoned => RunLifecycle::Parked`
(`crates/kontor-core/src/state.rs:862`), and `Parked` is in
`RunLifecycle::is_terminal` (`state.rs:672-677`). So parked *and* abandoned runs
are excluded.

Covered by `crates/kontor-daemon/tests/loopback_api.rs:9975`
(`a_terminal_agent_run_is_not_the_live_seat_of_its_still_open_team`), which
settles one seat of a multi-seat team, asserts the TeamRun stays open, then calls
`context:resolve` and asserts the selected seat is a *different, still-live* one —
exercising the agent-run half through a real API surface. The terminal *TeamRun*
half of the same fix has no dedicated test. Noted, not blocking.

## Findings

### F1 — P2 — a per-epic topology upgrade blocks placement for that epic — PRE-EXISTING, widened here — does not block

`crates/kontor-daemon/src/applications.rs:18246` (`ensure_task_node`) and
`applications.rs:4439` (`pin_epic_topology`).

`apply_topology_upgrade` (`applications.rs:9685`) repins **only** the epic
(`repin_mini_project_topology`, `crates/kontor-store/src/repository.rs:4199`) and
never moves the project's selected default. `project_topology`
(`applications.rs:18120`) reads only that default. Both sites above then refuse
when the epic pin `!=` the project default:
`placement_blocked / "this epic is pinned to another topology revision than the
project selects"`. So an *authorized, zero-effect* per-epic upgrade makes that
epic's tasks unplaceable.

This is not hypothetical, and the diff itself asserts the divergence is intended:
the new `topology_projection` code at `applications.rs:5009` reads the epic pin
precisely because "the two intentionally diverge after an authorized per-epic
upgrade".

Failure scenario (reproduced, exit 101): compose a project + epic + one task,
materialize the ticket scope (task node created, 200), publish v2 of the same
lineage whose only change is the ESW name template, upgrade-preview (zero
effects), upgrade-apply (200, pin → v2), then materialize the same task again →
`409 placement_blocked`.

**Attribution — I first had this as an OP-08 P1 and it is not.** OP-08 moves the
existing-task early return in `ensure_task_node` from function entry to after the
owning chain is ensured, which *does* newly expose `resolve_placement`
(`applications.rs:18015`) and `seat_replace` (`:15850`) to the check for
already-placed tasks. But running the same probe against `527cc16^1` produces the
identical refusal, there raised by `pin_epic_topology` via `ensure_scope_chain` —
so the epic was already dead for ticket materialization before this change. OP-08
widens the reach of a pre-existing defect; it is not the cause, and the ticket
never claimed to fix it.

Not covered by any test: the existing
`an_epic_pin_moves_only_through_the_preview_that_was_authorized`
(`loopback_api.rs:18254`) upgrades the pin and then only retitles and replays — it
never places anything afterwards. Recommend a follow-up ticket under ASMA-7869;
the likely fix is to judge placement against the *node's* pinned revision rather
than the project default.

### F2 — P3 — `staged_hop` does not require the hop to be inbound-compatible — introduced here — does not block

`crates/kontor-core/src/ticket.rs:1136-1160`. `staged_hop` gates on the reopen
selector being declared (`spec.class_of(...).is_some()`), directly reachable, not
where the ticket already stands, not the target itself, and reached by exactly one
live transition. It does **not** require the hop status to be in
`spec.inbound_compatible`.

`ExternalWorkflowSpec::validate` (`ticket.rs:868-880`) only requires `reopen` to be
a *declared* status, so the invariant is not enforced upstream either.

Failure scenario: a future pinned spec whose `reopen` is declared but absent from
`inbound_compatible`. Kontor hops the ticket onto it, and the very next
`reconcile` reaches `ticket.rs:1252` and returns `IncompatibleHumanMove`
permanently — Kontor having moved the ticket into a status its own policy refuses
to start from, then refusing to act on it.

Not live today: the shipped spec's `reopen` is `10213 READY FOR DEVELOPMENT`,
which *is* in `inbound_compatible` (verified against
`fixtures/external-workflow-asma.json`). Fix is one guard in `staged_hop`, or one
clause in `validate`.

### F3 — P3 — TeamRun lifecycle is last-child-writer-wins across sibling seats — introduced here — does not block

`crates/kontor-store/src/events/append.rs:299-350` (`reduce_team_lifecycle`).
Every fresh child observation drives the owning TeamRun's lifecycle through
`reduce_run_lifecycle` directly, so a team with one `running` seat and one
`waiting_input` seat reports whichever seat was observed last rather than an
aggregate over its children. With `send_message` now inspecting after every
message, that flapping is easy to reach.

Two things I expected to be problems here and checked, and they are not: no
client-facing surface takes a TeamRun `expected_revision` (all four internal uses
read the revision immediately before writing), so the extra revision churn cannot
cause spurious `revision_conflict` for callers; and SQLite's serialized writers
mean two sibling observations cannot race the CAS.

Projection semantics only. Worth a deliberate decision (aggregate vs. latest)
rather than a silent default.

### F4 — P3 — the doc says "fresh", the code never checks freshness — introduced here — does not block

`append.rs:296-303` documents `reduce_team_lifecycle` as reducing "the same fresh
child observation", and `reduce_run_lifecycle`'s own doc
(`crates/kontor-core/src/state.rs:777-785`) says "fresh runtime evidence".
`reduce_observation` has `freshness` in hand and passes it to `derive_run_state`,
but calls `reduce_run_lifecycle` (`append.rs:257`) without it. A stale observation
therefore still advances the lifecycle. Either check it or drop the word from both
doc comments.

### F5 — P3 — wall-clock microseconds as the monotonic reduction key — introduced here — does not block

`crates/kontor-runtime/src/observation.rs:37` (`timestamp_control_sequence`),
adopted at six adapter sites across `kontor-runtime-ao`, `kontor-runtime-codex`
and `kontor-runtime/src/fake.rs`.

`RunProjection::may_reduce` (`state.rs:1744`) requires strictly increasing
sequences, so a backward clock step (NTP correction) silently stops reduction: the
event is appended, the projection is not advanced, and nothing errors.

This is nonetheless a clear **improvement**, which is why it is P3 and not higher:
the previous constant `native_sequence: 0` made `may_reduce(Some(0), 0)` false
forever, so AO and Codex projections froze after their *first* observation. Paseo
already used timestamps (`kontor-runtime-paseo/src/adapter.rs:1934`).

Two loose ends: that Paseo site duplicates the new helper inline instead of
calling it; and the invariant that makes timestamp sequences safe — that no path
mixes them with real per-session sequences — is asserted nowhere. I verified it
holds today: only two `record_observation` call sites exist
(`applications.rs:2618` and `:17424`), both fed by a `ControlPlaneObservation`,
and AO's `observe_events` change-log sequences have no production consumer.

### F6 — P3 — `topology_inspect?epic_id=` now 404s for an unpinned epic — introduced here — does not block

`applications.rs:5009`. `topology_projection` with an epic now calls `epic_pin`,
which denies `NotFound / "this epic is not pinned to a topology revision yet"`.
Previously it fell back to `project_topology`, which *seeds* a default. The new
behaviour is arguably more honest, but it is an unannounced read-surface change
for an epic that exists and was never placed, and it is not mentioned in the
evidence docs.

### F7 — P3 — non-terminal `settle_runtime` reports `applied: created` on a replay — introduced here — does not block

`applications.rs:16281-16301`. The new non-terminal branch returns
`AppliedDto::Created` unconditionally, while the already-terminal branch two
screens up (`:16206`) correctly returns `Unchanged`. Replay-ness *is* determined
at `:16176` (`if let Some(existing) = self.replayed(...)`) but is not retained, so
the DTO cannot tell the two apart. Everywhere else in this codebase `applied`
describes the command receipt, so a replayed settle reporting `created` is
inconsistent with the rest of the surface.

## What I checked and found sound

Recording these because a post-hoc review that lists only complaints is not a
review, and several are the places I most expected to find defects.

- **Staged-hop policy is genuinely fail-closed.** `staged_hop` is not a path
  search. `ticket_policy.rs:1231`
  (`an_intermediate_kontor_was_not_given_is_refused_rather_than_invented`) covers
  four refusal shapes including the loop case (already standing on the hop) and
  the ambiguous case (two routes → `MultipleLiveTransitions`). I traced the
  hop → next-observation path by hand against the shipped spec: it converges or
  conflicts; it cannot ping-pong between two statuses.
- **`send_message`'s new mandatory post-send inspect is retry-safe.** A failed
  inspect returns an error *after* the message landed, which would duplicate the
  message if the runtime did not dedupe. It does: Paseo consults its ledger with
  `admit(&request.message_id, &body_hash)` *before* the wire
  (`kontor-runtime-paseo/src/adapter.rs:4515-4521`), and the API's idempotency key
  *is* the message id by contract (`kontor-api/src/sessions.rs:438`).
- **The new `Inspect` preflight cannot strand a runtime.** Every adapter that
  advertises `SendMessage` also advertises `Inspect` (paseo `SUPPORTED` and even
  `DEGRADED`; ao `SUPPORTED`; codex refuses `SendMessage` outright), so the added
  preflight introduces no refusal that `SendMessage`'s own preflight does not
  already impose.
- **Seat release is paired with every team closure.** All three
  `store.close_team_run(...)` sites (`applications.rs:1344`, `:1404`, `:15292`)
  are followed by `release_team_seats`, and the already-terminal replay branches
  (`:1359`, `:1381`, `:15273`) release too. This mattered because the same PR
  tightened `open_seat` (`:18672`) from "adopt any live holder" to
  `PlacementBlocked`, which would otherwise have stranded `(node, role slot)` keys
  permanently.
- **The replay-repair tests are not vacuous.**
  `replaying_ticket_materialization_repairs_a_missing_epic_control_plane` seeds the
  exact legacy shape by `DELETE`ing the ECP row directly in SQLite, then asserts
  the replay restores exactly one ECP *and* that the TSW's
  `observed_binding.native_id` is unchanged — logical repair without re-creating
  the native workspace.
- **`repository_roundtrip.rs:1551` changed an existing assertion** from `Queued`
  to `Running`. I checked this is a deliberate semantic change (lifecycle is now
  coupled to the observed dimension) rather than a test weakened to go green.
- **`reduce_team_lifecycle`'s `NotFound` branch is defensive only.**
  `agent_runs` has `FOREIGN KEY (project_id, team_run_id) REFERENCES team_runs`
  (`migrations/0001_init.sql:694`), so the missing-row path is unreachable.
- **The daemon-startup topology sweep fails closed by design.**
  `reconcile_mini_project_topology_nodes` (`repository.rs:1421`) revalidates every
  repair through the same hierarchy/capability proof as a fresh atomic repin and
  rolls the whole sweep back on any incompatibility, leaving
  `BarrierState::Failed`. Blast radius is realm-wide *scheduling* only
  (`barrier().state().is_open()` gates `applications.rs:14048` / `:14217`); reads
  still serve. Deliberate and documented — flagged as a known operational
  property, not a finding.

## Verdict

**PASS.**

Nothing this change introduced is a correctness defect that should block it, and
the substance the ticket claimed is really there and really tested: 1205 tests
green across all ten affected packages at the merge commit itself, with a clean
tree.

The one P2 (F1) is a pre-existing defect that OP-08 extends to one further
surface. I proved it pre-existing by running the same probe against `527cc16^1`
and getting the identical refusal — it deserves a follow-up ticket, not a revert
of this work. The remaining six findings are P3: two latent guards (F2, F5), one
projection-semantics decision (F3), one doc/code mismatch (F4), and two small
response-honesty issues (F6, F7).

Of the four prior findings, the two that were this change's to fix are closed with
tests that assert at the right level — the connector wire document for (a), a real
API surface for (d). The two still open ((b) and (c)) are in code this change does
not touch, and neither is live today.

I have recorded no Kontor gate. The LSA records it citing this verdict.
