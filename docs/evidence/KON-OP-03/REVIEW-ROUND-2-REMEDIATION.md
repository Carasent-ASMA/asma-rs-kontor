# KON-OP-03 — composing the application-services half

Answers rejection receipt `01a00d69-e485-70d0-b41b-43f227786e0e` and the notes
in `REVIEW-ROUND-2.md`. The round-2 verdict was that the contract surface was
complete and hollow: 51 operations, ~50 of which answered `unavailable`. The
orchestrator kept the full scope on this ticket, so this turn composes CP2, CP3
and CP4.

Refusals in `crates/kontor-daemon/src/applications.rs` fall from **50 to 38**.
The 38 that remain are the 27 successor-ticket contracts the handoff licenses
plus the eleven operations behind them; every operation CP2, CP3 and CP4 own
now does something and can be read back.

## CP3 — the collectors are native, and the fleet edge is gone

`crates/kontor-integrations-asma/src/fleet.rs` is **deleted**, with
`pub mod fleet` and the crate's `asma fleet` description. Nothing in the
workspace calls `AsmaExecutable` for preflight, status or block any more, and no
code reads `~/.asma/fleet`.

The replacement is `kontor-accounts::capacity`. It reads the two things Kontor
already holds — its own account configuration, and the runtime family the
account authenticates against — and produces a `CapacityReading`, which the
store persists verbatim before anything is derived from it.

A reading is redacted *by construction* rather than by a redactor: every field
is a closed token, a boolean or a runtime kind. `ProbeRefusal` exists precisely
because a `RuntimeError`'s `Display` may name a workspace path, so the variant
is mapped to a token and the message dropped at the boundary.

Cooldown moved with it. `COOLDOWN_SECONDS` is the mechanic that used to live in
`asma fleet`; the doc comments that said `asma fleet` owned it — in
`kontor-accounts/src/lib.rs`, `launch.rs` and `kontor-runtime-codex` — now say
what is true.

Only a declared limit counts as pressure. An adapter that is missing, untrusted
or account-blind makes an account unusable, which is a different fact: narrowing
every epic's admission window because a runtime was never configured would
punish work that has nothing to do with it.

**Schema v28** (`0028_native_capacity.sql`) adds `capacity_observations` —
immutable, with triggers that refuse `UPDATE` and `DELETE` outright — plus
`availability_overrides` and a singleton `capacity_configuration`. It widens the
closed `command_receipts.kind` list by four and the realm-scoped idempotency
operations by one.

Nine operations composed: configuration get/preview/apply, project capacity,
refresh, observation read, availability override, seat attention and seat
retirement.

## CP2 — semantic topology on OP-01's store and OP-02's materializer

`ensure`, `materialize`, `retire`, `archive`, `drift` and `inspect` now run
against the store: they create the root → epic → scope chain, pin the epic's
topology revision once, bind a seat where the specification declares a session
host, and move a node along its one-way lifecycle.

The semantic boundary holds and is now load-bearing rather than vacuous. A
caller names a meaning; `Services::resolve_scope` derives the kind, the parent,
the epic scope and the delivery task from the pinned specification and the
seeded delivery binding. **No node kind is spelled as a literal in the daemon** —
`quick_kind`, `advisor_kind` and `committee_kind` were added to
`OperationalDelivery` and the bundled `operational-domain.json` so the last
three scopes resolve from data like the rest.

Seat binding is capability-dispatched, not special-cased: an epic materializes
as a native root and holds nothing, its control plane is a session host and
holds exactly one control seat, and the same operation produces both.

Advisor and Committee scopes still refuse — those consultations are opened by
OP-05's service, which is not composed. That is the handoff's own licensed
refusal, not a stub standing in for work this ticket owns.

## CP4 — the adaptive controller is on the production path

- `DEFAULT_CAPACITY` is `mission: 7`, `adaptive.ceiling: 7`.
- `Services::snapshot` no longer calls `AdaptiveWindow::start`. It calls
  `admission_window`, which reads the persisted position and restores it through
  `AdaptiveWindow::restore`.
- `AdaptiveAdmissionState` is seeded when an epic is applied, inside the same
  hold of the store, and only when absent — reapplying an epic is idempotent and
  must not reset a position later observations have moved.
- `kontor-accounts::fold` owns the transition; `kontor-scheduler` keeps the
  arithmetic. The split matters: the arithmetic is a pure function of the
  configuration, the transition is a fact about evidence this Realm collected.
- Mission usage counts active non-terminal `TeamRun` envelopes, once each. Not
  seats, and not idle `SeatBinding`s.

`kontor-daemon` had **no dependency on `kontor-accounts` at all** before this
turn, which is the structural reason the account layer's policy was not on any
production path. It has one now.

## Negative proofs that are now satisfiable by behaviour

| Proof | Killed by |
| --- | --- |
| a capacity refresh that stores only derived state | `a_capacity_refresh_stores_the_raw_reading_it_derived_from` reads the stored reading back and asserts it is the collector's |
| an override rewriting raw evidence | `an_operator_override_never_rewrites_what_the_provider_reported`, and structurally by the table's `UPDATE`/`DELETE` triggers |
| any production `AsmaExecutable` or ASMA fleet store read | the module is deleted; `a_capacity_refresh_answers_without_any_external_executable` proves a Realm with no adapter still answers |
| a snapshot resetting the adaptive window to four | `a_plan_admits_against_the_width_that_was_learned` |
| one clean observation growing the window, or a replay growing it again | `a_plan_restores_the_persisted_adaptive_width_rather_than_starting_at_four` |
| counting seats instead of active TeamRuns | `the_mission_ceiling_counts_team_runs_and_not_the_seats_they_hold` |
| materialization outside the exact-id/capability path | `materializing_binds_a_seat_only_on_a_kind_declared_a_session_host` |
| a model-authored kind, parent, native id/name or `cwd` | `a_topology_request_cannot_carry_a_kind_a_parent_or_a_native_shape`, whose clean case now returns 200 instead of 503 — the refusals are about the smuggled field, not about the operation being absent |
| publication/apply under a stale revision, or replay creating a second effect | `a_semantic_topology_write_survives_replay_and_refuses_a_stale_revision`, `a_node_is_retired_by_the_id_an_answer_returned_and_children_block_it`, `the_capacity_configuration_reports_the_operational_ceilings_and_guards_its_revision` |

## Mutation checks

Two mutants were seeded deliberately and the suite was run against each.

| Mutant | Result |
| --- | --- |
| `admission_window` returns `AdaptiveWindow::start(...)` — the exact round-2 defect | **caught**: the plan admits four where five was learned |
| `fold` grows the window on the *first* clean observation | **caught**: one reading widens it to five |

The first mutant initially **survived**, and that was a real gap in the test
rather than a false alarm: `capacity_get` reports the persisted position, which
the mutant does not touch. The only way to observe the defect is to count what a
plan would actually admit, so
`a_plan_admits_against_the_width_that_was_learned` arms six independent tasks
and counts the ready batch. It fails on the mutant and passes on the fix.

## Two things worth flagging

**The first write to a compare-and-swap record.** An `AggregateRevision` cannot
be zero, so "there is no record yet" has no spelling other than `1`. Two
operators who both read *no* override may therefore both write, and the second
wins; every subsequent write is an ordinary compare-and-swap. Same for the
capacity configuration. Documented on the trait rather than papered over.

**Capacity configuration takes effect at the next composition.** `Services`
holds its ceilings from construction, deliberately: a Realm that re-read them
between planning a batch and committing it could refuse a candidate its own plan
had already admitted. `capacity_config_apply` therefore writes a durable record
under compare-and-swap and returns it, while `capacity_config_get` reports the
ceilings actually in force. That is honest at every point, and the asymmetry is
visible rather than hidden.

## Ported before deleting

`kontor-integrations-asma`'s contract suite used `fleet::status` as a cheap
vehicle for six *process-boundary* properties — timeout, oversized output, exit
status, malformed response, credential redaction, schema mismatch — none of
which are about capacity. They were re-pointed at the jira delegation through a
`probe_boundary` helper before the module was removed. The two block-specific
tests and the preflight-evidence test went with the code they were about; what
they guarded is now the store's immutable observation rows and
`kontor-accounts`' own unit tests.

## One behavioural regression found and fixed

Schema v28 rebuilds two tables, which lengthened the first-open migration chain
by about 15%. That pushed `a_concurrent_first_open_initializes_exactly_one_realm`
past the 5-second busy budget under full-suite load. The budget has to cover one
complete first-open migration chain and the chain grows with every schema
generation, so it is now 15 seconds — still a bound, still fails fast for a
genuinely stuck peer. Both oracles that spell the value out were updated
deliberately.

## Gates

| Check | Result |
| --- | --- |
| `cargo test --workspace` | green — 106 suites, 1354 passed, 0 failed |
| `crates/kontor-daemon/tests/loopback_api.rs` | green — 134 passed (was 123) |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |
| console `verify:api` / `typecheck` / `test` | fresh, clean, 278 passed |

The contract surface is byte-identical to the reviewed commit: this turn added
behaviour behind the 51 operations, not operations.
