# KON-OP-03 code-review gate — round 4 review notes

Reviewed: `f3d8b13` (super `71ca29b`; gitlink `f3d8b13` == submodule HEAD), the
three commits on top of `99c1e19`.
Rounds 1–3: `01a00d02-…`, `01a00d69-…`, `01a00deb-…` (all rejected).

Verdict: **passed** — receipt `01a00e19-61ee-73c0-825e-d7af66b437e4`, sequence 4.

## Mechanical checks — all green

| Check | Result |
| --- | --- |
| `cargo test --workspace` | green — 106 suites, 1361 passed, 0 failed, 8 ignored |
| loopback | green — 141 passed (+7) |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean, 0 violations |

## The round-3 gap is closed

All nine operations are composed against the real store. The refusal count in
`crates/kontor-daemon/src/applications.rs` falls from 38 to 29, and what remains
is exactly the licensed set: the 27 successor-ticket contracts, plus
`resolve_scope`'s two consultation variants. Nothing OP-03 owns refuses any more.

- `publish_topology_spec` reads the project at `expected_revision`, **revalidates**
  rather than trusting the supplied hash ("the hash proves the caller is
  publishing the document it had judged; it does not prove the verdict still
  stands"), and refuses a same-identity republish with different content before
  writing anything.
- `role` parses the code first, so something that could never be a code is
  refused as malformed rather than reported absent, then looks it up and answers
  `not_found` — never a guess.
- `code_help` reads the epic's actual pin, then that exact specification revision;
  an unpinned epic is `not_found` rather than an empty projection.
- The coherence gap is closed: `ensure_scope_chain`'s `pinned_spec` now has a
  `/v1` publish, read and upgrade path, and Admin kind-authority works.

**Migration 0029.** The blanket `mini_project_topology_snapshots_are_immutable`
trigger is replaced by a narrower one that still forbids re-parenting a pin, the
DELETE guard is untouched, and the command-kind CHECK is widened by the same
table-rebuild shape as v24/v28. The reasoning is recorded in the migration
itself: the specification revision stays immutable — which is the thing that
actually has to be frozen — while the pin becomes movable once, deliberately,
through the explicit preview/apply upgrade, audited in `command_receipts` rather
than in a second history table.

## CP1–CP4

| Checkpoint | State |
| --- | --- |
| CP1 — shared DTOs, authority/idempotency/preview rules, route table, OpenAPI, registry, generated artifacts | holds |
| CP2 — topology specification/read/upgrade, role catalog/code help, semantic topology | holds |
| CP3 — account-owned native capacity records/connectors, exact-seat operations, fleet `AsmaExecutable` edge removed | holds |
| CP4 — persisted adaptive controller, active-TeamRun accounting | holds |

## The 14 required negative proofs — all hold

| # | Proof | Evidence |
| --- | --- | --- |
| 1 | Route absent from `REGISTRY` and the probe list | parity oracle; `NON_AGENT_ROUTES` is health + OpenAPI only |
| 2 | Observer mutation or wrong minimum tier | tier oracle; `deciding_the_vocabulary_is_admin_authority_and_the_check_is_real`; `a_successor_contract_refuses_an_under_authorized_caller_first` |
| 3 | Stale revision; replay creating a second effect | `publishing_under_a_stale_revision_writes_nothing`; replay returns the original `receipt_id` with `applied: unchanged`; `a_semantic_topology_write_survives_replay_and_refuses_a_stale_revision` |
| 4 | Raw `role`, unknown role code, caller-supplied title | `deny_unknown_fields` on `TeamDraftRequest`; `the_catalog_resolves_a_known_code_and_refuses_an_unknown_one` (mutation-verified) |
| 5 | Model-authored kind/parent/native id/`cwd`/threshold/pid/argv | closed `SemanticTopologyTargetDto`; `resolve_scope` derives all of it |
| 6 | A published or epic-pinned specification changing in place | `a_published_specification_cannot_change_in_place` (mutation-verified); `an_epic_pin_moves_only_through_the_preview_that_was_authorized` |
| 7 | Materialization outside OP-02's exact-id/capability path | `materializing_binds_a_seat_only_on_a_kind_declared_a_session_host` |
| 8 | Refresh storing only derived state; override rewriting raw evidence | `a_capacity_refresh_stores_the_raw_reading_it_derived_from`; `an_operator_override_never_rewrites_what_the_provider_reported` |
| 9 | Production `AsmaExecutable`, fleet store/event read, AgentsRoom description | `fleet.rs` deleted; `a_capacity_refresh_answers_without_any_external_executable` |
| 10 | Snapshot resetting the adaptive window to four | `a_plan_restores_the_persisted_adaptive_width_rather_than_starting_at_four`; `a_plan_admits_against_the_width_that_was_learned` (mutation-verified) |
| 11 | One clean observation growing the window; replay growing it again | mutation-verified in `99c1e19` |
| 12 | Counting seats instead of TeamRuns; an eighth run; cancelling under pressure | `the_mission_ceiling_counts_team_runs_and_not_the_seats_they_hold` |
| 13 | `epic-get` Team revision immediately and after restart | pre-existing, closed by `5f95fa1` |
| 14 | Contract-only successor reporting success | `every_successor_contract_refuses_rather_than_answering_emptily` |

## What remains refusing, and why that is correct

- **27 successor-ticket contracts** (Core Team, Quick work, Promotion, Epic
  roster, Advisor and Committee configuration and runs, Completion configuration
  and runs). The handoff assigns their behavior to OP-04/05/06 and prescribes a
  typed `unavailable` before any effect until the owning service is composed.
- **`resolve_scope`'s Advisor and Committee variants.** An `AdvisorRunId` or
  `CommitteeRunId` exists only once OP-05 opens the run, so there is no aggregate
  to scope a node to.

## On the review process

Both round-1 placeholder tests were rewritten into behavioural tests rather than
deleted, so no test is left asserting a refusal that is no longer true. Across
rounds 3 and 4 the builder seeded four mutants and reported them honestly,
including one that initially escaped and the two defects its own tests found. The
commit messages record the decisions — apply names a preview rather than a
target; candidate identity hashed from the parsed document, not the bytes; a
collision judged before the rules — at the level a reviewer needs.

The `/v1` contract surface and the Operational application services this ticket
owns are complete. Passing to `qa`.
