Artifact: resolver-coverage-mutants

# ASMA-8116 resolver coverage — seeded and killed mutations

Each mutation was applied alone to `crates/kontor-store/src/jira.rs`, the named
test run, and the file restored byte-for-byte afterwards. No mutation was
retained; `cargo test -p kontor-store --test jira_materialization` is 20/20 at
the end.

| # | Mutation | Test | Result |
| --- | --- | --- | --- |
| M1 | Project scoping dropped from the resolver's epic arm (`binding.project_id = ?1` → `(binding.project_id = ?1 OR 1=1)`) | `resolution_is_project_scoped_and_never_reaches_a_foreign_projects_binding` | **killed** |
| M2 | Ambiguity guard disabled (`if rows.len() > 1` → `if false`) | `an_ambiguous_cross_ledger_key_is_a_typed_conflict_rather_than_a_backend_error` | **killed** |
| M3 | Typed uniqueness mapping deleted (`unique_conflict` body reduced to `backend(error)`) | `a_uniqueness_violation_establishing_an_immutable_issue_is_a_typed_conflict` | **killed** (see below) |

## What M3 exposed, and why the first two attempts at it did not count

M3 initially **survived** twice, and both survivals were real findings rather
than noise.

1. Run against `two_issues_racing_for_one_key_settle_on_exactly_one_typed_winner`,
   it survived because that test never reaches the ledger's uniqueness index at
   all: `reconcile_confirmed_jira_key` holds an application pre-check that
   refuses the loser before any write is attempted. The race proves one winner
   and a typed loser, but it does not prove the *constraint* path.
2. Re-aimed at a deterministic constraint violation, it still survived — because
   `backend` already converts any `ConstraintViolation` into
   `Conflict { subject: "storage" }`. Asserting merely that the refusal is *a*
   conflict therefore passes whether or not the domain mapping exists.

The test now asserts the refusal's **subject and rule**, not just its variant.
With M3 applied it fails with
`Conflict { subject: "storage", rule: "a uniqueness, check or immutability constraint refused the write" }`
against the expected `"confirmed Jira binding"`. That is the distinction scope
item 4 asks for: a caller learns which rule refused and what to do about it,
rather than that some constraint somewhere said no.

The path exercised is `establish_immutable_issue_id`, which is the one write
with no application pre-check in front of it — nothing has claimed the id yet,
so only the ledger's own uniqueness can refuse a second subject claiming it.

## Prior mutation, still valid

`BINDING-VALIDATION-MUTANT.md` records the shared cross-subject guard being
disabled and killed by
`activation_requires_every_confirmed_binding_and_survives_readback`.

## Gate-1 repair mutations

| # | Mutation | Test | Result |
| --- | --- | --- | --- |
| M4 | Cross-ledger check removed from `establish_immutable_issue_id` | `a_legacy_exact_replay_cannot_claim_an_issue_already_bound_in_the_other_ledger` | **killed** |
| M5 | Return value resolved after commit again | `a_rename_reports_what_it_committed_even_when_the_row_moves_underneath_it` | **killed**, 3/3 deterministic (a fixture-only trigger replaces the unschedulable timing window; the earlier contention test could not kill it and was recorded as survived) |
| M6 | Canonical key-change trigger reverted to the two-row agreement | `two_direct_sql_updates_cannot_forge_the_tail_of_a_proven_rename` | **killed** |
| M7 | `follow_same_issue_rename` unwired from `reconcile_jira_epic` | `the_resident_reconciler_follows_a_same_issue_rename_through_the_connector` | **killed** |

All seven mutations are killed against the current head. Every one was applied
alone, the named test run, and the source restored byte-for-byte.

## Gate-2 repair mutations

| # | Mutation | Test | Result |
| --- | --- | --- | --- |
| M8 | Epic identity refusal made non-terminal | `the_resident_reconciler_refuses_a_key_that_now_answers_for_another_issue` | **killed** |
| M9 | Task identity refusal made non-terminal | `the_resident_reconciler_gives_no_effect_to_a_task_key_that_moved_issue` | **killed** |
| M10 | Rename authority unbound from the predecessor key | `a_stale_task_rename_authority_cannot_restore_an_earlier_destination` | **killed** |

M8 and M9 each survived one earlier assertion shape; see
`GATE-2-REPAIR-REPORT.md` for what each survival exposed about the test.

