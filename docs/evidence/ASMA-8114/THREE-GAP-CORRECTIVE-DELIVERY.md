# ASMA-8114 three-gap corrective delivery — delivered unit evidence

Status: frozen candidate on `feat/ASMA-8114-three-gap-corrective-delivery`,
rebased onto module default `6e0b3557ec5270f12694fc3b3beb9de0942c6caa`
(ASMA-8196 MCP parity #286 on ASMA-8234 refusal diagnostics #285 on ASMA-8196
roster occupancy #284 on the yoke-derive refresh #283); this is the third and
final refresh, with the slot-1 and slot-2 lanes merged. The initial delivery
was based on `f95e206563bca88b6871f48528623441e5e1a231` (PR278). Nothing was
pushed, merged, deployed, restarted or materialized; no Jira, Keychain,
credential or runtime effect was exercised. Qualification and review are
dispatched separately. The rework that answers the independent LSA
qualification of `083232ca` is recorded in the final section of this document.

## Commits

| Commit | Concern |
| --- | --- |
| `94286eaa` | Gap 1 — legacy item-code topic refusal independent of template rendering |
| `e3294722` | Gap 2 — transactional duplicate refusal parity (typed store error) |
| `e350432f` | rustfmt of the changed source and tests |
| `9e8733ee` | Gap 3 — task catalog checkout binds slug and branch to the task key |
| (this document) | evidence record |

## Gap 1 — legacy item code is forbidden regardless of the template

- Changed: `crates/kontor-daemon/src/applications.rs`
  - `derivable_item_code_for_subject` (line 40490) derives the code whenever the
    epic has an active immutable backlog code, returning `None` otherwise.
  - `item_code_for_subject` (line 40513) is the strict wrapper a rendering
    template still uses.
  - `consultation_semantic_identity` (line 40534) now derives the code
    unconditionally (line 40577), refuses `placement_blocked` only when the
    pinned container template renders an item-code token and no code exists, and
    always adds a derived code to `forbidden_scope_fragments` for both Advisor
    and Committee.
- Behavior after: a topic containing the derived Kontor item code is refused
  `invalid_request` / `consultation_topic_repeats_scope_code`,
  `about(ConsultationTopic)`, `located_at("topic")`, with zero run rows and zero
  fake-runtime calls, even when the pinned Team Definition renders
  `SCOPE_JIRA_KEY` instead of the item code. An epic with no active immutable
  backlog code gains no new refusal.
- Behavioral witness:
  `a_legacy_item_code_is_forbidden_in_a_topic_even_when_the_template_does_not_render_it`
  (`crates/kontor-daemon/tests/loopback_api.rs:60366`) pins the ASMA-8117
  Jira-key successor, derives the real code from the store, and drives both
  families; a clean topic then succeeds as `CSW • <epic key> • operational
  completion`, proving the template really does not render the code.

## Gap 2 — transactional duplicate refusal parity

- Changed:
  - `crates/kontor-core/src/repository.rs:212` — `RepositoryError::
    DuplicateConsultation { family, run_id }`, deliberately distinct from the
    generic `Conflict`.
  - `crates/kontor-store/src/repository.rs:179` — `is_duplicate_semantic_
    identity` matches only the declared `consultation_runs_by_semantic_identity`
    constraint (or its column tuple). At line 2446 the insert result is captured;
    on that constraint the losing transaction is rolled back (line 2496), the
    committed survivor is read by semantic identity, and the typed error naming
    it is returned (line 2501); an unexpectedly invisible row falls back to the
    pre-existing generic conflict.
  - `crates/kontor-api/src/error.rs:645` — maps the variant to exactly the
    sequential shape: code `idempotency_conflict`, rule
    `consultation_semantic_duplicate: this <Advisor|Committee> scope and topic
    already has one run`, `about("consultation semantic identity")`,
    `located_at("consultation-runs/<existing run id>")`, advising `read or
    resume the existing consultation run`.
- The unique index remains authoritative and the insert atomic; no other
  uniqueness, check or immutability conflict is reclassified.
- Store witness (forced concurrency, two real writers with a barrier):
  `concurrent_semantic_identity_losers_get_the_typed_duplicate_naming_the_
  survivor` (`crates/kontor-store/tests/team_definition_persistence.rs:1098`)
  races Advisor and Committee pairs; exactly one wins per identity and the loser
  receives the typed refusal naming the survivor, with its own topology node
  rolled back and no run row.
- Daemon witness:
  `a_fresh_key_cannot_freeze_the_same_semantic_consultation_concurrently`
  (`crates/kontor-daemon/tests/loopback_api.rs:60551`) joins two invokes per
  family with different keys and one semantic identity: exactly one `200`, one
  `409` carrying rule, locator to the winning run and the read/resume action,
  exactly one run per family, and only the winner's seats reaching
  `LaunchConsultation`.
- The existing sequential test now also asserts the typed variant and surviving
  run (`crates/kontor-store/tests/team_definition_persistence.rs:1038`).

## Gap 3 — task catalog checkout binds to the confirmed task key

- Changed: `crates/kontor-runtime-paseo/src/checkout.rs`
  - `ManagedBranchBinding` (line 27) now carries confirmed task keys separately
    from the full confirmed set; `from_scopes` (line 42) collects both without
    changing the epic-key behavior.
  - `ensure_catalog_identity` (line 105) is the single catalog attestation used
    by `verify_catalog_module_checkout` (line 304): when the scope names a task,
    the slug must be a confirmed task key and the actual canonical branch must
    carry one; an epic key cannot stand in. Epic-level scopes keep the existing
    epic-or-task binding, and `verify_checkout`/`ensure_creatable` (branch-
    encoded path, `place_task_branch`) are untouched.
  - An absent catalog checkout stays refused with the existing
    `WorkspacePreparationFailed` rule.
- Behavioral witnesses:
  - Unit: `a_task_catalog_checkout_binds_both_its_slug_and_branch_to_the_task_
    key` (line 489) and `an_epic_level_catalog_checkout_keeps_its_epic_or_task_
    binding` (line 513), plus the extended
    `asma_catalog_path_is_recognized_by_its_exact_confirmed_key_slug` and
    `actual_catalog_branch_must_be_canonical_and_bound`.
  - Contract: `preparation_refuses_a_task_catalog_branch_bound_to_the_epic_key`
    (`tests/contract.rs:3260`), `preparation_refuses_an_epic_catalog_slug_for_a_
    task_checkout` (`:3290`), `preparation_refuses_an_absent_catalog_module_
    worktree` (`:3319`); `preparation_accepts_an_asma_managed_catalog_module_
    worktree` (`:3160`) and `preparation_accepts_the_epic_key_as_the_default_
    branch_identity` stay green. Every refusal asserts no workspace creation.

## Baseline bracket and post-change green

Pre-change (base `f95e206563bca88b6871f48528623441e5e1a231`, clean tree apart
from untracked adapter files):

- `cargo test -p kontor-core -p kontor-store -p kontor-api -p kontor-jira -p
  kontor-accounts` → 65 test binaries, **1122 passed, 0 failed, 0 ignored**.
- `cargo test -p kontor-runtime-paseo` → **126 unit + 285 contract passed**,
  6 ignored (opt-in live).
- Daemon targeted filters (10): `a_seeded_committee_runs_and_settles_instead_of_
  returning_503`, `committee_containers_follow_their_recorded_subject_not_their_
  caller`, `consultation_containers_follow_their_recorded_subject_not_their_
  caller`, `jira_key_containers_render_the_confirmed_binding_of_their_own_
  subject`, `a_consultation_with_no_recorded_subject_refuses_to_be_named`,
  `a_kickoff_hold_lifts_itself_at_the_boundary_that_satisfies_its_terms`,
  `jira_link_apply_recovers_a_mixed_pending_batch_in_place`,
  `identical_mixed_jira_apply_resumes_its_pending_create_in_place`,
  `a_missing_or_wrong_credential_is_refused`,
  `the_credential_file_is_owner_only` → **10/10 passed**.

Post-change (frozen HEAD):

- Same combined command → 65 binaries, **1123 passed, 0 failed, 0 ignored**
  (+1: the new forced-concurrency store test).
- `cargo test -p kontor-runtime-paseo` → **128 unit + 288 contract passed**,
  6 ignored.
- `cargo test -p kontor-daemon` (whole package) → 123 + 7 + 5 + 12 + 485 + 2 +
  21 + 6 + 3 = **664 passed, 0 failed, 1 ignored**.
- `cargo fmt --all -- --check` → exit 0.
- `cargo clippy --workspace --all-targets -- -D warnings` → exit 0.

## Mutation witnesses (compiled, current baseline)

Each mutation was applied to the frozen source, the named regression run, the
bytes restored from HEAD, and the regression re-run green.

| Mutation | Named regression | Result |
| --- | --- | --- |
| Added `.filter(\|_\| renders_item_code)` to the item-code fragment push | `a_legacy_item_code_is_forbidden_in_a_topic_even_when_the_template_does_not_render_it` | FAILED, exit 101: the invoke returned 200 and the title carried the legacy code (`CSW • ASMA-236403797 • CE-236403797 operational completion`); restored → passed |
| `is_duplicate_semantic_identity` returns `false` | `concurrent_semantic_identity_losers_get_the_typed_duplicate_naming_the_survivor` | FAILED, exit 101: loser received `storage conflict: a uniqueness, check or immutability constraint refused the write`; restored → passed |
| `ensure_catalog_identity` forced to the epic-or-task binding | Contract `preparation_refuses_a_task_catalog_branch_bound_to_the_epic_key` and unit `a_task_catalog_checkout_binds_both_its_slug_and_branch_to_the_task_key` | FAILED, exit 101 each (refusal absent; unit `matches!` assertion failed); restored → both passed |

These are new current-baseline executions on this candidate. Historical
attribution is preserved exactly: the e29 `MUT-TOPIC` / `MUT-UNIQUE` kills are
exact-e29 executions, and the exact-1b qualification transfers to 173d by
executable, test and API byte equality — it is not a distinct 173d execution.
Neither receipt was re-run, relabelled or cited as this candidate's evidence.

## Zero-effect and naming/redaction evidence

- Refused topics: the Gap 1 test asserts the fake call count is unchanged and
  zero consultation runs exist after both family refusals.
- Duplicate losers: the store test asserts the loser's topology node is absent
  and exactly one run exists; the daemon test asserts one run per family and
  that only the winner's seats reach the runtime.
- Catalog mismatches: each Gap 3 contract test asserts `workspace create` count
  is zero (and, for the absent case, that the directory was never synthesized).
- Refusal text: the Gap 1 test asserts the 400 body does not contain the
  caller's item code; the rule/subject/at are the existing static conventions.
- Hash distinctness/stability: the existing
  `logical_consultation_identity_ignores_retry_mechanics_and_changes_with_
  semantics` (`crates/kontor-core/tests/consultation_specs.rs:211`) proves
  distinct topics and re-review provenance hash distinctly while the same input
  is stable; it stays green.
- Advisor/Committee parity is exercised by both Gap 1 families, both Gap 2
  store races, and both Gap 2 daemon races.

## Compatibility

- **PR278 credential scope (f95e2065)**: `cargo test -p kontor-jira` → 19 unit
  + 2 `credential_scope` + 20 `native_connector` passed, 0 failed;
  `kontor-accounts` and `kontor-store` (`realm_preflight`) passed in the
  combined run; the PR278-touched daemon cases (`a_kickoff_hold_lifts_itself_at
  _the_boundary_that_satisfies_its_terms`, both `jira_link_apply`/mixed-apply
  cases, and the two credential tests) passed 5/5 post-change. No waiver.
- **Migration/backup/schema**: no migration, schema or data file changed. The
  full store run covers `schema_v1`, `backup_export`, `backup_snapshot`,
  `team_definition_backup_contract`, `realm_preflight` — all green.
- **API/CLI/MCP**: no contract growth; `kontor-api` (`error_envelope`,
  `openapi_contract`) and `kontor-daemon` (`mcp_journey` and the registry-bound
  loopback suite) are green. `RepositoryError` is `#[non_exhaustive]`; the new
  variant is additive.

## Preservation statement

This delivery changed only Rust source and test files. It did not touch any
database, migration, backup, credential, Keychain or Jira artifact; it started
no daemon or runtime and invoked no real consultation or materialization. The
three historical KTHSR consultations, all nine findings, their identities, and
the archived `50cf` state were not read, written, renamed, retitled, archived or
otherwise touched. The pre-existing `docs/evidence/ASMA-8114/HIGH-SCOPE-
RECORD.md` and `OPERATIONAL-GAPS.md` remain unmodified.

## Rework after LSA qualification (`083232ca` → final HEAD)

The independent qualification of `083232ca87b0fd0d0d789e8849291d24378628ef`
returned `does-not-qualify`: no P0, one P1 (Gap 3 catalog identity), two P2
(Gap 2 daemon coverage; committed-export gate pending). Corrections:

| Commit | Concern |
| --- | --- |
| `0b871236` | P1 — one shared task key across catalog slug and branch; task presence tracked |
| `da0acbb2` | P2 — deterministic daemon insert-loser envelope and exact API mapping regression |

### P1 — catalog identity (LSA-8114-01)

- `crates/kontor-runtime-paseo/src/checkout.rs`: `ManagedBranchBinding` now
  tracks `task_present` separately from the parsed `tasks` (line 27); a
  task-bearing scope whose key is absent or non-canonical refuses with
  `binding_unconfirmed` instead of falling back to the epic.
  `ensure_catalog_identity` (line 107) selects the one task key whose lowercase
  spelling equals the slug and requires the actual canonical branch to carry
  that same key. Epic-level scopes and the branch-encoded
  `ensure_creatable`/`verify_checkout` paths are unchanged.
- Regressions added: `a_catalog_checkout_binds_its_slug_and_branch_to_one_
  shared_task_key` (`checkout.rs:561`) and `a_task_scope_without_a_confirmed_
  task_key_cannot_be_treated_as_epic_level` (`checkout.rs:590`); `preparation_
  refuses_a_catalog_branch_bound_to_a_different_task_key` — the LSA two-key
  counterexample — (`tests/contract.rs:3290`) and the coherent-key positive
  `preparation_accepts_a_catalog_checkout_on_the_configured_task_key`
  (`tests/contract.rs:3320`). Each refusal asserts zero workspace creates.
- Witness: changing `task_present = true` to `false` made all three named
  regressions fail exit 101 (unit shared-key, unit presence, contract two-key);
  bytes restored, all green.

### P2 — transactional transport witness (LSA-8114-02)

- `crates/kontor-api/tests/error_envelope.rs`:
  `the_transactional_duplicate_carries_the_exact_sequential_envelope`
  (line 212) constructs `DuplicateConsultation` for both families and asserts
  code/status/rule/subject/at/action and that the generic `Conflict` still maps
  to `revision_conflict` without leaking the internal subject.
- `crates/kontor-daemon/tests/loopback_api.rs`: `concurrent_daemon_insert_
  losers_render_the_exact_sequential_envelope` (line 60710) races two real
  writers through
  the daemon realm's own store (barrier, scoped threads), asserts exactly one
  winner and the typed loser naming the survivor, then feeds that exact store
  error through the realm's transport mapping (`ApiError::from_repository`)
  and asserts the same exact envelope for both families, plus loser node
  rollback and one run per family. The joined endpoint race now asserts exact
  rule and subject equality and a null `current_revision`.
- Witness: changing only the `DuplicateConsultation` mapping to
  `RevisionConflict` made both new tests fail exit 101; bytes restored, both
  green.

### Committed-export gate (LSA-8114-03)

`scripts/verify-tree.py --mode archive` was mirrored gate-for-gate on
`git archive HEAD` of the corrected candidate. Exact results:

| Gate | Command | Exit |
| --- | --- | --- |
| lock | `cargo metadata --locked --format-version 1` | 0 |
| fmt | `cargo fmt --all -- --check` | 0 |
| clippy | `cargo clippy --workspace --all-targets -- -D warnings` | 0 |
| tests | `cargo test --workspace --locked -- --skip system_keychain_reports_a_missing_entry_as_not_found` | 0 (2953 passed, 0 failed, 9 ignored) |
| audit | `cargo audit` | 0 (0 vulnerabilities; 10 allowed warnings) |
| deny | `cargo deny check` | **1 — pre-existing** |
| js install | `pnpm install --frozen-lockfile` | 0 |
| js types | `pnpm -r typecheck` | 0 |
| js tests | `pnpm -r test` | 0 (305 passed) |
| js audit | `pnpm audit --prod` | 0 |

Two exact-facts exceptions, no waiver claimed:

1. The one `--skip` is `system_keychain_reports_a_missing_entry_as_not_found`
   (`crates/kontor-accounts/tests/account_security.rs:1947`): it queries the
   real macOS Keychain. The rework constraint forbids Keychain access
   whatsoever, so the suite was run without it; the independent qualification
   excluded the same test with the same exact `--skip`.
2. `cargo deny check` fails on `yoke-derive 0.8.3` (yanked; `advisories
   FAILED`). The identical command on the base export
   `f95e206563bca88b6871f48528623441e5e1a231` fails identically, so it is
   pre-existing in the untouched lockfile and not attributable to this
   candidate.

The full `python3 scripts/verify-tree.py --mode archive` entry point was
therefore not invoked as a single command; every gate it runs was executed on
the same archive export with the exact results above.

### Rebase onto `4d365dd5` and reruns

The frozen candidate was rebased cleanly (all 8 commits replayed, no conflicts)
onto module default `4d365dd5de649c68aaabe8c2b67351ae7dd5dbb0` (PR283). On the
rebased tree:

| Check | Command | Exit |
| --- | --- | --- |
| fmt | `cargo fmt --all -- --check` | 0 |
| clippy | `cargo clippy --workspace --all-targets -- -D warnings` | 0 |
| deny | `cargo deny --offline --locked check` | 0 (`advisories ok, bans ok, licenses ok, sources ok`) |
| combined | `cargo test --offline --locked -p kontor-core -p kontor-store -p kontor-api -p kontor-runtime-paseo` | 0 (1382 passed, 0 failed, 6 ignored) |
| daemon witness 1 | `a_legacy_item_code_is_forbidden_in_a_topic_even_when_the_template_does_not_render_it` | 0 |
| daemon witness 2 | `a_fresh_key_cannot_freeze_the_same_semantic_consultation_concurrently` | 0 |
| daemon witness 3 | `concurrent_daemon_insert_losers_render_the_exact_sequential_envelope` | 0 |
| mutations | G1 item-code, G2 store duplicate, G3 catalog binding, P2 mapping | 101 under each mutation; bytes restored; all green |

One timing nuance: the first combined run had a single failure in the
unrelated, time-bounded `client::tests::an_oversized_canonical_socket_answer_
fails_without_waiting_for_timeout` under load; it passed in isolation and the
full rerun above is exit 0.

### Slot-3 refresh onto `c93b1e43` and reconciliation

The candidate was rebased with `git rebase --onto c93b1e43… 4d365dd5…
feat/ASMA-8114-three-gap-corrective-delivery` (all 9 commits replayed). **No
textual conflicts occurred**: the default's changes in
`crates/kontor-daemon/src/applications.rs` (roster read port),
`crates/kontor-api/src/error.rs` (refusal diagnostics) and
`crates/kontor-daemon/tests/loopback_api.rs` sit away from the lane hunks, so
every lane commit re-applied additively on the merged content.

Reconciliation was semantic, and exactly one test hunk needed it:
`crates/kontor-api/tests/error_envelope.rs` —
`the_transactional_duplicate_carries_the_exact_sequential_envelope`'s
generic-conflict arm now asserts the merged refusal-diagnostics contract
(`RevisionConflict` naming its own static subject/rule) instead of the former
withheld text, while still asserting no consultation-run locator. The lane's
own `DuplicateConsultation` envelope assertions are unchanged.

Content statements after the refresh:

- Files the default did not touch are byte-identical to the previously
  qualified bytes: `crates/kontor-core/src/repository.rs`,
  `crates/kontor-store/src/repository.rs`,
  `crates/kontor-store/tests/team_definition_persistence.rs`,
  `crates/kontor-runtime-paseo/src/checkout.rs`,
  `crates/kontor-runtime-paseo/tests/contract.rs` (verified by an empty
  `git diff ef6a5a2e..HEAD` over those paths).
- The three shared files are reconciled content, not blob-equal: the merged
  slot-1/2 changes are preserved intact and the three-gap corrections are
  re-applied on top. The lane diff `c93b1e43..HEAD` contains only the lane's
  hunks; the only removed default lines are the lane's own refactor and
  replacement lines.
- `Cargo.lock` and `Cargo.toml` are identical to the default
  (`git diff c93b1e43..HEAD -- Cargo.lock Cargo.toml` empty).

Reruns on the refreshed tree:

| Check | Command | Exit |
| --- | --- | --- |
| fmt | `cargo fmt --all -- --check` | 0 |
| clippy | `cargo clippy --workspace --all-targets -- -D warnings` | 0 |
| deny | `cargo deny --offline --locked check` | 0 (`advisories ok, bans ok, licenses ok, sources ok`) |
| combined | `cargo test --offline --locked -p kontor-core -p kontor-store -p kontor-api -p kontor-runtime-paseo` | 0 (1384 passed, 0 failed, 6 ignored) |
| openapi | `cargo test --offline --locked -p kontor-api --test openapi_contract` | 0 (3 passed) |
| envelope | `cargo test --offline --locked -p kontor-api --test error_envelope` | 0 (6 passed) |
| contract crate | `cargo test --offline --locked --no-fail-fast -p kontor-tests-contract` | 101 — pre-existing merged-default gap, below |
| daemon witness 1 | `a_legacy_item_code_is_forbidden_in_a_topic_even_when_the_template_does_not_render_it` | 0 |
| daemon witness 2 | `a_fresh_key_cannot_freeze_the_same_semantic_consultation_concurrently` | 0 |
| daemon witness 3 | `concurrent_daemon_insert_losers_render_the_exact_sequential_envelope` | 0 |
| mutations | G1 item-code, G2 store duplicate, G3 catalog binding, P2 mapping | 101 under each mutation; bytes restored; all green |

Merged-default gap found while running the regenerated contract suites, not
introduced by this lane: `crates/kontor-tests-contract/tests/mcp_parity.rs`
fails `every_documented_operation_is_mapped_once_or_allowlisted_once` and
`the_snapshot_canary_holds_at_this_base` (10 passed / 2 failed) because the
default's two new operations
`GET /v1/projects/{project_id}/epics/{epic_id}/core-team` and `GET
/v1/projects/{project_id}/epics/{epic_id}/core-team/seats/{seat_binding_id}/occupancies`
have neither an MCP tool nor an allowlist entry (195 documented operations
against the frozen 193 canary). The identical suite fails the same way on a
clean `git archive c93b1e43` export (exit 101, 10 passed / 2 failed), so the
parity decision belongs to the merged lane, not to this reconciliation. All
other contract-crate binaries pass (guardrails 5, mcp_cardinality 11,
mcp_mutants 11, profiles_teams 11, runtime_adapter 52, scheduling 2).

### Third and final refresh onto `6e0b3557`

The candidate was rebased with `git rebase --onto 6e0b3557… c93b1e43…` (all 10
lane commits replayed, no conflicts). This is the third and final refresh: the
default now contains the merged slot-1 and slot-2 lanes plus the PR #286 MCP
parity fix. Lane diff `6e0b3557..HEAD` is the same expected 10 files (1774
insertions, 136 deletions); `Cargo.lock` and `Cargo.toml` are identical to the
default; the parity fix is intact (`crates/kontor-mcp/src/registry.rs`
mappings present, `tests/contract/mcp_parity.rs` canary
`documented().len() == 195`).

Reruns on the refreshed tree:

| Check | Command | Exit |
| --- | --- | --- |
| fmt | `cargo fmt --all -- --check` | 0 |
| clippy | `cargo clippy --workspace --all-targets -- -D warnings` | 0 |
| deny | `cargo deny --offline --locked check` | 0 (`advisories ok, bans ok, licenses ok, sources ok`) |
| combined | `cargo test --offline --locked -p kontor-core -p kontor-store -p kontor-api -p kontor-runtime-paseo` | 0 (1384 passed, 0 failed, 6 ignored) |
| contract crate | `cargo test --offline --locked --no-fail-fast -p kontor-tests-contract` | 0 — `mcp_parity` 12 passed; the merged-default parity gap found in the previous slot-3 refresh is closed by PR #286 |
| daemon witness 1 | `a_legacy_item_code_is_forbidden_in_a_topic_even_when_the_template_does_not_render_it` | 0 |
| daemon witness 2 | `a_fresh_key_cannot_freeze_the_same_semantic_consultation_concurrently` | 0 |
| daemon witness 3 | `concurrent_daemon_insert_losers_render_the_exact_sequential_envelope` | 0 |
| mutations | G1 item-code, G2 store duplicate, G3 catalog binding, P2 mapping | 101 under each mutation; bytes restored; all green |

## Notes and unresolved findings

- Merged-default contract gap (closed): the `mcp_parity` failures observed on
  `c93b1e43` in the previous refresh are fixed by PR #286 in `6e0b3557`; the
  contract crate now passes `mcp_parity` 12/12 on the final refresh.
- The joined endpoint race may resolve through the sequential pre-check on a
  current-thread runtime; the transactional insert-loser is now deterministically
  witnessed through the daemon realm store and its exact transport envelope for
  both families.
- The evidence logs for every command above were captured locally during this
  delivery; this document records their exact commands and results.
- Process note: the local formatting commit `e350432f` was amended once from an
  AI-generated subject that named Gap 3 while the staged diff was formatting
  only. Nothing was pushed; the content is unchanged and the message now names
  the formatting concern.
