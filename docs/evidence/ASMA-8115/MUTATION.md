# ASMA-8115 / PUB-07 mutation ledger

Date: 2026-09-06
Includes inherited ASMA-8101 / PUB-01 acceptance repair.

Each mutant was applied alone with `apply_patch`, observed red under its named
test, restored with `apply_patch`, and observed green. No mutant remains.

| Mutant | Killing regression | Red evidence | Restored readback |
| --- | --- | --- | --- |
| Treat every repository as approved | `a_publication_for_an_unbound_repository_is_refused` | Exit 101; refusal reasons became empty instead of `repository_mismatch` | Exit 0; 1 passed |
| Resolve only the first Jira-key match | `a_branch_key_resolving_to_an_epic_and_task_is_refused_as_ambiguous` | Exit 101; refusal reasons became empty instead of `binding_ambiguous` | Exit 0; 1 passed |
| Skip every placement-admission refusal | `every_unconfirmed_placement_fact_refuses_with_its_exact_reason` | Exit 101; `JiraBindingUnconfirmed` was admitted without an attestation digest | Exit 0; 1 passed |
| Let description recovery also overwrite Jira summary | `explicit_link_updates_only_description_and_reads_it_back` | Exit 101; the exact description-only PUT contract was violated | Exit 0; 1 passed |
| Invert the current-head SHA comparison | `a_current_github_head_refuses_a_truly_stale_publication_sha` | Exit 101; a truly stale SHA returned `Ok(())` | Exit 0; 1 passed |
| Let scheduler launch rematerialize its accepted container | `explicit_materialization_places_an_unconfigured_project_before_scheduler_launch` | Exit 101; the adapter recorded two `PrepareContainer` effects after planning | Exit 0; 1 passed |

## Re-verification after the ASMA-8110/8123/current-master integration

Date: 2026-09-18. Base `ae8b401f` (`ASMA-8111 Select an exact Committee
completion result (#236)`); the change was rebased off its original
`508a5141` base and its migration renumbered to schema v100.

Every mutant above was applied again, alone, against the rebased tree, observed
red under its named test, restored with `git checkout --`, and observed green.
No mutant remains in the tree.

| Mutant | Red | Restored |
| --- | --- | --- |
| Treat every repository as approved | `FAILED. 0 passed; 1 failed` | `ok. 1 passed` |
| Resolve only the first Jira-key match | `FAILED. 0 passed; 1 failed` | `ok. 1 passed` |
| Skip every placement-admission refusal | `FAILED. 0 passed; 1 failed` — expected `Some(JiraBindingUnconfirmed)` | `ok. 1 passed` |
| The description-only PUT also writes summary | `FAILED. 0 passed; 1 failed` | `ok. 1 passed` |
| Invert the current-head SHA comparison | `FAILED. 0 passed; 1 failed` | `ok. 1 passed` |
| Let scheduler launch rematerialize its accepted container | `FAILED. 0 passed; 1 failed` — two `PrepareContainer` effects recorded after planning | `ok. 1 passed` |

## Re-verification on the ASMA-8101 coherent candidate

Date: 2026-09-19. Base `d5fef521` — `1569c184` (ASMA-8234 generic correlation)
→ `b54ba141` (ASMA-8110 salvage carry) → `d5fef521` (ASMA-8049 identity repair,
patch-ID identical to `0df3da03`). Schema 105; PUB-07's migration is v106.

Each mutant was applied alone, run with correctly tokenized cargo arguments,
observed red through its own named test's assertion, restored by exact file
copy, and observed green. `cmp` proves every touched source byte-identical
afterwards; no mutant remains.

| Mutant | Killing test | Red | Green | Restore |
| --- | --- | --- | --- | --- |
| Treat every repository as approved | `a_publication_for_an_unbound_repository_is_refused` | 101 | 0 | exact |
| Resolve only the first Jira-key match | `a_branch_key_resolving_to_an_epic_and_task_is_refused_as_ambiguous` | 101 | 0 | exact |
| Skip every placement-admission refusal | `every_unconfirmed_placement_fact_refuses_with_its_exact_reason` | 101 | 0 | exact |
| The description-only PUT also writes summary | `explicit_link_updates_only_description_and_reads_it_back` | 101 | 0 | exact |
| Invert the current-head SHA comparison | `a_current_github_head_refuses_a_truly_stale_publication_sha` | 101 | 0 | exact |
| Let scheduler launch rematerialize its accepted container | `explicit_materialization_places_an_unconfigured_project_before_scheduler_launch` | 101 | 0 | exact |

Six `test result: FAILED` lines were recorded across the red runs and no CLI
argument-parse error, so each red is the test detecting the mutation rather
than a malformed invocation.
