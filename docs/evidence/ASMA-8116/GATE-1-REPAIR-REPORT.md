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

## Two honest qualifications

### F-8116-V6: the fix is structural, the mutation is not killable here

`reconcile_confirmed_jira_key` no longer resolves after committing; it derives
subject, key, evidence and revision inside the transaction that wrote them, so
the window the verifier identified cannot open. The retained test asserts every
committing caller is told its own key, and treats `NotFound` as a failure —
which is the exact shape the old code produced.

Re-applying the old post-commit read (mutation M5) does **not** fail that test,
across 40 rounds × 2 writers × 3 attempts. SQLite serializes writers, so the
window between one caller's commit and its own read is microseconds while a
competing writer needs milliseconds to acquire the lock and finish. The defect
was real by inspection and the fix removes it by construction; the timing is
simply not reachable from a test in this harness. This is recorded rather than
presented as a kill.

### F-8116-V3: the wiring costs one extra Jira read per confirmed subject per pass

The rename pass asks the connector for each confirmed subject's current identity.
That is one GET per subject per reconciliation pass, and two existing gates
correctly detect it:

- `automatic_jira_reconciliation_records_and_resolves_an_unfinished_held_epic_without_effects`
  — `issue_reads` 2, expected 1.
- `resident_jira_conflict_replay_waits_for_the_bounded_backstop`
  — `issue_reads` 6, expected 4.

Those assertions protect a real invariant: the resident loop must not hammer
Jira, and an unchanged durable conflict must wait for the bounded backstop.
**They were not relaxed.** Loosening them to make this branch green would trade
a verified property for a green tick.

The correct fix is to reuse the read the resident loop already performs rather
than issuing a second one: `reconcile_jira_epic` already calls `observe()`, whose
`Observed` carries the whole raw connector answer. It cannot be used as-is
because `JiraResponse::issue_key` is echoed from the *request*, not read from the
response body, and `JiraResponse` carries no immutable id at all. Threading the
observed key and id through the observation payload — in `kontor-jira`'s response
types and both reconcile paths — makes the identity check free and removes the
extra read.

That work is identified but not performed here, so this repair round leaves two
red daemon gates. It is reported rather than hidden, and no test was weakened to
conceal it.

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
