Artifact: gate-repair-report

# ASMA-8116 — repair of high-verification gate sequence 1

Gate receipt: `01a09557-a9dc-7031-a4be-be59efcf81f9`
Verifier report: `HIGH-VERIFICATION-REPORT.md` (commit `0a400ec`, preserved)

## Finding-by-finding

| Finding | State | Evidence |
| --- | --- | --- |
| F-8116-V1 legacy replay binds one issue to two subjects | **closed** | cross-ledger check moved *before* legacy id establishment, inside the transaction; `a_legacy_exact_replay_cannot_claim_an_issue_already_bound_in_the_other_ledger` covers epic→task and task→epic; mutation M4 killed |
| F-8116-V2 two direct SQL updates forge a rename | **closed** | rename authority ledger added; trigger rebound to it; `two_direct_sql_updates_cannot_forge_the_tail_of_a_proven_rename` runs the full two-statement sequence in one raw transaction; mutation M6 killed |
| F-8116-V3 reconciliation unreachable from production | **closed, with a cost — see below** | `Services::reconcile_confirmed_jira_keys` wired into `reconcile_jira_once`; `JiraConnector::observe_identity` added; two end-to-end daemon tests (same-id rename followed, different-id refused); mutation M7 killed |
| F-8116-V4 connector suite red, new readback untested | **closed** | 5 accepted fixtures given the mandatory top-level `id`; exact-id assertion added; four missing/malformed-id refusals added; `cargo test -p kontor-jira` 9 + 19 green |
| F-8116-V5 no v94→v95 preservation gate | **closed** | `v95_adds_immutable_jira_identity_without_losing_or_inventing_a_single_v94_row` builds the real v94 shape, runs the actual `0095`, and asserts row preservation, NULL identities, project-scoped uniqueness, id immutability, trigger installation/replacement, integrity and reopen |
| F-8116-V6 committed rename reported as failure | **fixed structurally; mutant not killable** — see below | the answer is now built inside the committing transaction; `a_committed_rename_is_never_reported_as_a_failure_by_its_own_caller` retains the invariant under sustained contention |

## Continuation round: V3 completed with zero extra reads, V6 deterministically proven

### F-8116-V3 — the extra Jira read is gone

The first repair wired reconciliation through a second request
(`observe_identity`), which cost one GET per confirmed subject per pass and
turned two bounded-read gates red. That approach is withdrawn and the method
removed. Identity now rides the read the boundary already performs:

- `LiveIssue` captures the identity from the `?fields=*all` answer `live()`
  already fetches.
- `JiraResponse` gained `observed_identity`, kept deliberately distinct from
  `issue_key`. The latter is **echoed from the request**, so it still reads
  `ASMA-1` after Jira has renamed that issue; comparing the two is what
  distinguishes a rename from a steady state. Inferring one from the other was
  the trap.
- `reconcile_jira_epic` compares the stored immutable id with the observed one
  and calls `follow_same_issue_rename`, which reconciles a same-id key change
  and refuses a different-id answer. No request of its own.

Both previously red gates are green again, unmodified:
`automatic_jira_reconciliation_records_and_resolves_an_unfinished_held_epic_without_effects`
and `resident_jira_conflict_replay_waits_for_the_bounded_backstop`. Neither
assertion was relaxed. The invariant is now asserted directly rather than left
implicit: `the_resident_reconciler_follows_a_same_issue_rename_through_the_connector`
measures the reads of a renaming pass and of a steady-state pass and requires
them equal.

Two observe-path connector fixtures then had to supply a top-level `id`, for the
same fail-closed reason as F-8116-V4: a readback without an observable identity
is malformed, and `live()` now says so.

### F-8116-V6 — the mutant is killed deterministically

The production construction is unchanged: the answer is still derived inside the
committing transaction. What changed is the proof.

`a_rename_reports_what_it_committed_even_when_the_row_moves_underneath_it`
installs a trigger **in the fixture only** that rewrites the key immediately
after the reconciling update commits it. That reproduces, without timing, the
one thing the post-commit window makes true: the key the caller committed is not
the key the table holds when a later read happens. Nothing in production is
relaxed to arrange it.

Re-applying the old post-commit reread now fails the test on every attempt
(3/3), where the earlier contention-based test could not kill it in 40 rounds ×
2 writers × 3 attempts. The previous round's report recorded that survival
rather than claiming a kill; this round replaces it with a real one.

## Preserved, not reinterpreted

- **OQ-002** — the intermittent `schema_v1` concurrent-open failure. Passed this
  round (58/58, including the new gate). A pass does not settle the rate.
- **OQ-004 / `mcp_journey`** — still fails with
  `placement_blocked: the epic has no active immutable backlog code`; assigned to
  ASMA-8117 / 8119 integration.
- **Migration numbering** — this branch and ASMA-8117 both claim `0095`. Retained
  because internally coherent; final number not guessed.
- **`JiraItemCode` and the legacy-field prohibition** — untouched.
  `crates/kontor-core/src/backlog_identity.rs` and its tests remain byte-identical
  to `origin/master`.
