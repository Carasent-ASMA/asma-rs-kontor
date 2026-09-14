Artifact: gate-repair-report-round-2

# ASMA-8116 — repair of high-verification gate sequence 2

Gate receipt: `01a0959b-6637-7c80-b788-64a238a164f6`
Verifier evidence hash: `75f45e118d6f2977208943e82509a99ce516ab772c12b40b519d6fc9fbd2424b`
Verifier report: `HIGH-VERIFICATION-ROUND-2.md` (commit `a01eee2`, preserved)
Repaired candidate: `0913786` plus this round.

Round-1 findings V1, V4, V5 and V6 stay closed and untouched.

## F-8116-V7 — task bindings now enter the production identity path

The identity decision was epic-only: its sole call site was inside
`reconcile_jira_epic`, and its lookup selected `JiraBindingSubject::Epic`.

It is now one gate applied to both ledgers. `decide_jira_identity` takes the
subject, and the task path calls it inside `prepare_ticket_plan` — placed
between `observe()` and `plan()`, so it precedes every conflict record, intent
and connector apply, and protects all three of that method's callers rather
than the resident loop alone.

Two end-to-end task tests were added beside the epic pair:
`the_resident_reconciler_follows_a_same_issue_task_rename` and
`the_resident_reconciler_gives_no_effect_to_a_task_key_that_moved_issue`.

## F-8116-V8 — an identity refusal is now terminal

`follow_same_issue_rename` returned `()`, so its caller could not act on the
outcome and continued into classification, policy and a possible apply. It is
replaced by `decide_jira_identity`, which returns `IdentityDecision`:

- `Proceed { current_key, renamed }` — identity proven. The caller continues
  **under the key Jira reports now**, so no later intent or effect carries the
  superseded request key.
- `Stop` — a different immutable id, a binding whose id was never retained, an
  answer carrying no identity, or a failed reconciliation. Fail-closed in all
  four cases: the epic path returns a `Blocked` verdict immediately and the task
  path refuses the plan.

The regressions were rebuilt to discriminate this, which the previous ones did
not. The fixture now puts the issue in `DRAFT` with a live transition to
`TO BE GROOMED` and counts every non-GET, so a subject that continued would
genuinely try to take it. The epic test additionally asserts
`content_conflicts == 0` and `blocked == 1`: under the rejected behaviour the
subject reached content classification and recorded a conflict about an issue
the Realm had just proved was not its own. The task test asserts the plan is
refused *by the identity gate's own rule text*, not merely that it failed.

## F-8116-V9 — rename authority is transition-exact and non-replayable

Authority named only a destination, so any historical row for the link and
immutable id admitted that destination forever. After A→B→C, raw SQL could
reuse the A→B row to roll back to B.

Authority now names the predecessor as well: `from_external_issue_key` is part
of the row and of its primary key, and the canonical trigger requires
`authority.from_external_issue_key = OLD.external_issue_key`. An authority
therefore describes one *transition* from one current state. After A→B→C the
A→B row matches nothing, and restoring B needs fresh readback authorizing C→B.

Authority is recorded only for an actual transition: re-confirming the key a
binding already holds moves nothing, so there is no transition to authorize.

**The epic ledger gained the same protection**, because it had none at all — a
raw update could rename a confirmed epic binding to any value.
`jira_epic_rename_authorizations` and
`jira_epic_bindings_key_change_requires_proof` mirror the task mechanism.

Retained three-state regressions:
`a_stale_task_rename_authority_cannot_restore_an_earlier_destination` performs
ASMA-2 → HISTORIC-2 → CURRENT-2, then proves a raw two-statement rollback to
HISTORIC-2 is refused while a freshly proven forward rename still succeeds.
`a_stale_epic_rename_authority_cannot_restore_an_earlier_destination` proves the
same for epics, including a key nobody ever authorized.

## Mutations seeded, killed and restored

| # | Mutation | Test | Result |
| --- | --- | --- | --- |
| M8 | Epic identity refusal made non-terminal (counted, then continued) | `the_resident_reconciler_refuses_a_key_that_now_answers_for_another_issue` | **killed** |
| M9 | Task identity refusal made non-terminal | `the_resident_reconciler_gives_no_effect_to_a_task_key_that_moved_issue` | **killed** |
| M10 | Authority no longer bound to the predecessor key | `a_stale_task_rename_authority_cannot_restore_an_earlier_destination` | **killed** |

M8 and M9 each survived a first attempt, and both survivals were findings about
the *tests* rather than the code: the first assertions could not tell a correct
refusal from a scenario that had nothing to do. M8 was closed by asserting the
classification the subject must never reach; M9 by asserting the exact rule that
must refuse the plan. Neither was recorded as a kill until it was one.

M1–M7 from earlier rounds remain killed against this head.

## Preserved

Zero-extra-read behaviour (the identity still rides the observation the boundary
already performs, asserted directly), exact typed refusals, `JiraItemCode` and
the legacy token/code semantics — `crates/kontor-core/src/backlog_identity.rs`
and its tests remain byte-identical to `origin/master`. OQ-002, OQ-004 and the
ASMA-8117 migration-number collision keep their existing ledger attribution; no
number is guessed and ASMA-8117 is untouched.
