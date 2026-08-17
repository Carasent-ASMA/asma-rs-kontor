# KON-OP-04 code review

Date: 2026-08-17
Status: changes requested
Scope: the OP-04 composition in `090b61f`, reviewed against `ARCHITECTURE.md`
Seat: inspector (`code` work profile, `code-review` phase)

Companion to `ARCHITECTURE.md` and `IMPLEMENTATION.md`. That pair decides and
records; this one reports what the build does under failure, which is the only
part of the architecture's promise the happy-path tests do not reach.

## Verdict

Two blocking defects, both in the same place: the ordering between a durable
reconciliation key and the effects it is supposed to reconcile. Everything else
in this change is sound, and several parts of it are better than the contract
required.

Both defects share a shape. The commit message states the mechanism correctly —
"the promotion row is written before the first effect, carrying the ids the
effects will use" — and migration `0031` states it again in prose. Promotion
gets the epic id right and the roster wrong; `ensure_quick_session` gets it
wrong outright. Neither is caught by a test, because every new test drives the
success path.

## Blocking

### 1. A partially applied promotion can never be resumed, and its source can never be promoted again

`crates/kontor-daemon/src/applications.rs:6767` freezes the roster *after*
`create_mini_project` (`:6740`) and both `ensure_scope_chain` calls (`:6752`,
`:6756`). The resume path reads that roster before anything else
(`:6678` → `frozen_roster` → `NotFound` at `:2399`).

So if the first apply fails anywhere between `begin_promotion` (`:6709`) and
`freeze_roster`, every retry takes the `Some(promotion)` branch at `:6670` and
dies on a roster that was never written. `quick_session_promotions` is keyed by
`quick_session_id` (migration `0031`, `:69`) and the build exposes no delete,
abort or reset, so the Quick session is permanently unpromotable.

The trigger is not exotic. `ensure_scope_chain` performs native placement and
returns `placement_blocked` in precisely the situations OP-02 exists to detect —
parent, kind, `cwd`, identity or readback disagreement — plus any runtime
outage. An operator who hits one of those, fixes it, and retries gets a
permanent refusal on a resource that no longer has anything wrong with it.

This is also an undeclared deviation from the architecture, which orders the
promotion transaction explicitly: freeze the Core Team revision at step 2,
create the MiniProject at step 4, materialize at step 6, create seats at step 7
(`ARCHITECTURE.md:220`). The build runs step 2 after steps 4–6.
`IMPLEMENTATION.md` names two places the build could not follow the
architecture; this is a third, and unlike the other two it has no
justification — the roster is already resolved at `:6682`, before the effects
begin.

Fix: move `freeze_roster` into the `None` branch beside `begin_promotion`, where
the roster is already in hand. `epic_rosters` has no foreign key to
`mini_projects`, so the write is legal before the MiniProject exists.

### 2. `ensure_quick_session` writes its reconciliation key last, so a failure orphans a QSW node and its seat

`applications.rs:6586-6623` creates the topology node, then the seat binding,
then the `quick_sessions` row — three separate transactions, because
`with_store` is a mutex around one store call and nothing more
(`crates/kontor-api/src/state.rs:416`).

That row is the *only* thing that can reconcile a retry. The code deliberately
refuses to find the node by search, and says why at `:6559`: two Quick sessions
in one project are both QSW nodes below the same base, so a search cannot tell
them apart. Correct — and it means that until the row exists, the node and seat
are unattributable. A crash or store error after `:6588` leaves both behind, and
the retry mints fresh ids at `:6546-6547` and places a *second* QSW under the
same base.

It does not take a crash. The intent hash covers project, role, catalog and
purpose, but not the idempotency key (`:6518-6525`), so two concurrent ensures
with the same role and purpose under different keys both pass the
`quick_session_for_intent` check at `:6532`, both create a node and a seat, and
the loser fails `UNIQUE (project_id, intent_hash)` — leaving an orphaned node
and an unattached seat binding.

An unattached seat binding is the exact artefact that produced the OP-REQ-039
phantom, so this failure mode has already cost this system real time once.

Fix: write the `quick_sessions` row first, mirroring `begin_promotion`. Its
`topology_node_id` and `seat_binding_id` columns are plain `TEXT` with no
foreign key (migration `0031`, `:23-25`), so the row can carry the ids before
the effects exist — which is what the migration's own comment says the mechanism
is.

## Non-blocking

### 3. The lead-architect role code is a literal in the daemon

`applications.rs:6772` finds the LSA seat with `role_code.as_str() == "LSA"`,
three lines from code that reads `domain.delivery.control_role_code` and
`domain.delivery.quick_kind` from configuration. It is self-consistent today
because `kontor-teams/src/operational.rs:107` inserts the same literal, but the
codes reference calls the Operational vocabulary published specification data
rather than a kernel enum. An operator whose catalog spells the role differently
gets "the frozen roster has no LSA seat to hand the work to" — a `placement_blocked`
that names a role their catalog never had.

### 4. A failed roster upgrade reports the next attempt as someone else's edit

`apply_roster_upgrade` freezes before materializing (`:6892` then `:6903`), which
is the right order. But `put_epic_roster` increments the revision on conflict, so
a failure during materialization leaves the pin moved and the caller's next
attempt refused with `revision_conflict` — "the epic's roster moved since the
caller previewed it", when in fact their own failed attempt moved it. Recoverable
by re-reading and re-previewing, and additive-only materialization makes the
retry safe, so this is ergonomics rather than a defect. Worth a comment.

### 5. `IMPLEMENTATION.md` overstates where PSW mismatch is enforced

It says mismatch "is enforced, at both ensure and promotion". Enforcement lives
in `promotable` (`:2364`); `session_base` at ensure time only refuses a base that
does not exist (`:2231`), because at ensure there is no prior observation to
disagree with. The behaviour is right and the reasoning in the doc is right — the
sentence is not.

Separately, the resume path skips `promotable` entirely (`:6677`), so the drift
check does not run on a resumed promotion. Defensible, since the remaining
effects sit under the ESW rather than the PSW, but it is worth stating rather
than leaving as an accident of control flow.

## What holds

Recorded because these were the parts most likely to be got wrong, and were not:

- previews genuinely commit nothing — no draft, no id, no receipt — and both
  applies recompute the plan and hold it to the digest the caller was shown
  (`:6317`, `:6691`);
- the handoff is delivered before success is reported, and a resumed apply
  re-attempts delivery on `completed_at` rather than assuming it (`:6784`);
- `materialize_roster_seats` reconciles by role slot and skips seats already
  held, so it is safe to re-run and preserves seat identity (`:2499`);
- roster upgrade is additive only, and never retires a seat that left the
  project's roster (`:6900`);
- both migrations follow the v24/v28/v29 command-receipt rebuild precedent
  exactly, and the immutability triggers on `core_team_revisions` match the
  topology specification's;
- the `CoreTeamSeatSelectionDto` correction the architecture required as a
  precondition landed, and the OpenAPI contract was regenerated with it —
  verified in `crates/kontor-api/contract/openapi.json`;
- the `not_found`-rather-than-empty-roster refusal is the right call and is
  tested.

## Evidence

- `cargo test --workspace --no-run` — clean.
- `cargo test --workspace` — see the run recorded below.
- 13 tests added in `crates/kontor-daemon/tests/loopback_api.rs`, all ten
  successor routes exercised at least once.
- Coverage gap behind both blocking findings: no test drives a failed or
  interrupted apply. `a_quick_session_is_opened_once_and_replays_to_the_same_ids`
  covers the lost-acknowledgement replay where the first call *succeeded*; the
  architecture's required proof is "across duplicate, lost-ack and restart
  retries" (`ARCHITECTURE.md:305`), and the restart arm is unproven for both
  `quick-sessions:ensure` and `promotion:apply`.

## Gate

`code-review-gate`: **not passed** — changes requested on findings 1 and 2.

Both are ordering changes of a few lines each, in code whose surrounding
comments already describe the correct behaviour. Each needs a test that fails
before the fix: a promotion whose first apply is interrupted after
`begin_promotion` and resumes, and an ensure whose `quick_sessions` write fails
after the node exists.
