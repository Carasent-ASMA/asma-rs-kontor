# ASMA-8237 open questions

Date: 2026-09-20
Task: Jira `ASMA-8237`
TeamRun: `01a0bfd6-cb03-7160-a381-3d561b48e2bd` / TSW `wks_f4bc61a339794e3a`
Governing record: `high-scope-record-asma-8237-20260920` rev 1,
revision_id `01a0bfde-c2fe-7051-a52b-8b6d5a080a40`,
approval receipt `01a0bfde-d8c0-70d1-b2f3-078d038ddfec`.
Worktree: `asma-modules/.worktrees/asma-8237/asma-rs-kontor`, HEAD
`9f61c74ad4c0ca859e4e4f665cb9a72c37a30fd2`.

No gate is passed by this document. It records one resolved acceptance-evidence
question and one newly opened question that this seat did **not** resolve.

## OQ-001 — acceptance evidence for "refusal for models the runtime does not list" — resolved

- **Attaches to:** ASMA-8237 acceptance criteria and commit `9f61c74a`
  ("Advertise approved watchdog fallback routes"). The question itself was
  recorded upstream in the governing high-scope record; this entry records only
  its resolution and the evidence behind it.
- **Former ambiguity:** the ticket asks for refusal of models the runtime does
  not list. The delivered regression
  (`the_model_catalog_preserves_watchdog_route_and_effort_boundaries`) proves
  only that three guessed aliases are *absent from the advertised catalog*. It
  was not established whether absence from the advertised catalog is itself the
  enforced refusal, or whether a separate request-level refusal exists.
- **Options observed:** (a) catalog omission is the enforced boundary, so
  existing governed request-rejection evidence should be returned rather than
  new tests written; (b) omission is only advertisement, so a focused
  request-level refusal test is owed.
- **Resolution — (b).** Omission from the advertised catalog is *not* the
  enforced boundary. Two distinct lists exist:
  - **Advertised:** `Services::model_catalog`
    (`crates/kontor-daemon/src/applications.rs:16448`), served at
    `GET /v1/catalog`. This is what `9f61c74a` changed.
  - **Enforced:** `model_route_is_catalogued`
    (`crates/kontor-daemon/src/applications.rs:12581`), a separate hardcoded
    allowlist consulted on write paths. `9f61c74a` did not touch it.

  A governed request-level refusal does exist:
  `parse_runtime_model_route` (`:12773`) returns
  `DomainError::invalid("RuntimeModelRouteRequest", "the model route is not in
  the governed catalog")` at `:12790`; a second site refuses a team draft rung
  at `:12858`. No pre-existing test asserted either refusal — grepping both
  rule strings across `crates/` and `tests/` returned only the source sites.
  There was therefore no existing evidence to return, and option (b) applies.
- **Evidence delivered:** the smallest focused request-level refusal test,
  `requesting_a_model_the_runtime_does_not_list_is_refused_by_the_governed_catalog`
  (`crates/kontor-daemon/src/applications.rs:37291`). It asserts the three
  guessed aliases are refused with the exact governed reason, and carries one
  admitted control (`codex`/`gpt-5.6-sol`) so the refusals cannot pass for the
  wrong reason.
- **Mutation verification (both directions caught):**
  - `_ => false` → `_ => true` in `model_route_is_catalogued`: refusal
    assertion failed as required (`left: Ok(ModelRung { .. "glm-5.3-flash" })`).
  - dropped `gpt-5.6-sol` from the listed codex arm: admitted control failed as
    required (`left: Err(Invalid { .. "not in the governed catalog" })`).
  - Reverted; `cargo test -p kontor-daemon --lib` 82 passed / 0 failed;
    `--test loopback_api model_catalog` 2 passed / 0 failed;
    `cargo fmt -p kontor-daemon` and `cargo clippy -p kontor-daemon --lib
    --tests` clean.
- **Rejected reductions:** do not treat advertised-catalog absence as proof of
  refusal, and do not widen the focused test into an effort-ceiling or
  admission-policy assertion — see OQ-002.

## OQ-002 — every route `9f61c74a` advertises was refused by the enforced allowlist — resolved by bounded repair

- **Attaches to:** ASMA-8237, commit `9f61c74a`, and the enforced predicate
  `model_route_is_catalogued`
  (`crates/kontor-daemon/src/applications.rs:12581`).
- **Former ambiguity:** `9f61c74a` added six advertised routes to
  `model_catalog`, but `model_route_is_catalogued` had no arm matching any of
  them, so each fell to `_ => false`. Measured directly against the predicate,
  all six returned `admitted=false` — identical to the three invented aliases
  the catalog regression was written to exclude. That predicate gates
  `provider_fallbacks` admission (`crates/kontor-daemon/src/runtimes.rs:558`),
  team-draft rungs (`:12855`), committee template slots (`:14951`) and seat
  recovery (`:23723`).
- **Options observed:** (a) intended separation — advertising is informational
  and admission is a separate approval; (b) defect — the advertised routes are
  unnameable on every write path; (c) some path admits them without consulting
  the predicate, which this seat did not find.
- **Re-verified against current `origin/master` before repair:** the scope
  record captured `origin_master` as `e9184291`; current `origin/master` is
  `0f649824`, two commits ahead. `git show origin/master:...` returns a
  byte-identical `model_route_is_catalogued` with no arm for any of the six
  routes, so the mismatch still exists on current master.
- **Resolution — (b), by bounded direct repair** under the user's explicit
  direct-repair authority conveyed by the coordinating root. The six exact
  approved provider/model routes are now recognizable at exactly the advertised
  effort ceilings:

  | route | admitted efforts |
  |---|---|
  | `codex`/`codex-work`/`codex-personal` `gpt-5.6-luna` | low, medium, high, xhigh, max |
  | `cursor/auto-smart` | low, medium, high, xhigh |
  | `opencode/openrouter/z-ai/glm-5.3-flash` | low, high, max |
  | `opencode/openrouter/nvidia/nemotron-3-ultra-550b-a55b:free` | medium, high |

- **Deliberately not changed — these gates remain exactly as they were:**
  account headroom, role eligibility, provider availability
  (`provider_available`), template authority, and the approved watchdog chain.
  The repair touches recognition only. `default_model_for_provider` was **not**
  given a `cursor` entry, so cursor cannot become a family fallback default.
  Invented aliases (`glm-5.3-flash`, `nemotron-3-ultra:free`, `cursor-auto`)
  remain refused, and efforts above each advertised ceiling remain refused —
  including the invented Nemotron `max`.
- **Not claimed:** the watchdog was not started or restored, schedule `809e6aec`
  was not touched, and no claim is made that automatic pre-start failover
  works. The approved memory's recorded limitation on automatic pre-start
  switching stands unchanged; this repair makes the chain's rungs *nameable*,
  which is necessary but not sufficient for failover, and failover itself
  remains unevidenced.
