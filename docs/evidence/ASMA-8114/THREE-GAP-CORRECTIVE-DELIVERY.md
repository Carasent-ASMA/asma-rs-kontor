# ASMA-8114 three-gap corrective delivery — delivered unit evidence

Status: frozen candidate on `feat/ASMA-8114-three-gap-corrective-delivery`,
based on module default `f95e206563bca88b6871f48528623441e5e1a231` (PR278).
Nothing was pushed, merged, deployed, restarted or materialized; no Jira,
Keychain, credential or runtime effect was exercised. Qualification and review
are dispatched separately.

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

These are new current-baseline executions. The historical e29 `MUT-TOPIC` /
`MUT-UNIQUE` and exact-1b/173d executions are preserved history and were not
re-run, relabelled or cited as this candidate's evidence.

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

## Notes and unresolved findings

- On a current-thread test runtime the daemon-level joined race may resolve
  through the sequential pre-check; the transactional loser is deterministically
  witnessed by the store-level two-writer test. Both produce the identical
  refusal shape, which the daemon test asserts.
- The evidence logs for every command above were captured locally during this
  delivery; this document records their exact commands and results.
- `scripts/verify-tree.py --mode archive` (the full export gate, including
  network advisory checks and the JavaScript gates) was not run for this
  scoped Rust-only candidate; the applicable Rust gates were run directly and
  are recorded above.
- Process note: the local formatting commit `e350432f` was amended once from an
  AI-generated subject that named Gap 3 while the staged diff was formatting
  only. Nothing was pushed; the content is unchanged and the message now names
  the formatting concern.
