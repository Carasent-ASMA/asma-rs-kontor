# High Audit Report — ASMA-8116

- Artifact: `high-audit-report`
- Decision: **rejected**
- Audited candidate: `527decf58cfcd68ad69f60dad5a5d30a3551ac83`
- Candidate tree: `231d0e4873d0e4ba2db2d50b8c2b0a403d7c8b6d`
- Base and merge-base: `e40b5f42759c06c15ea8d666a18b843d950db51d`
- Scope record: `docs/evidence/ASMA-8116/HIGH-SCOPE-RECORD.md`
- Open-question ledger: `docs/evidence/ASMA-8116/OPEN-QUESTIONS.md` (`OQ-005`)

## Decision basis

The candidate does not meet the accepted invariant that a previously consumed
rename authorization cannot become current again. The occurrence counter is
documented as monotonic, but schema v95 only constrains it to be non-negative.
Both authorization guards then trust that rewritable counter when selecting a
historical authorization.

The audit was performed from an isolated archive of the exact candidate. Live
working-tree edits made after that commit were excluded.

## F-8116-A1 — high: a sequence rewind reactivates spent rename authority

Schema v95 adds `rename_sequence` to both confirmed-binding ledgers with only
`CHECK (rename_sequence >= 0)`
(`crates/kontor-store/migrations/0095_immutable_jira_issue_identity.sql`, exact
candidate lines 16–28). A raw update can therefore lower either counter. The
task guard accepts an authority whose `from_rename_sequence` equals the mutable
confirmation counter (lines 127–145); the epic guard accepts one whose sequence
equals the mutable binding counter (lines 181–194).

Two independent verifier probes exercised the full supported key cycle
`A -> B -> C -> A`, rewound only the sequence to `0`, and attempted the original
historical `A -> B` transition:

| Ledger | Expected | Candidate result |
| --- | --- | --- |
| task | historical authority refused | update succeeded; assertion failed |
| epic | historical authority refused | update succeeded; assertion failed |

Command:

```text
CARGO_TARGET_DIR=/private/tmp/asma-8116-audit-target.YwCd0A \
  cargo test -p kontor-store --test jira_materialization verifier_ -- --nocapture
```

Result: **failed**, 0 passed / 2 failed. The failures were
`verifier_task_sequence_rewind_must_not_reactivate_historical_authority` and
`verifier_epic_sequence_rewind_must_not_reactivate_historical_authority`.

Impact: anyone able to execute the same direct ledger writes already covered by
the migration's database-boundary threat model can replay an old same-issue
rename proof. The append-only authorization tables remain intact, but their
occurrence selector can be moved backward, so append-only storage no longer
means single-use authority.

Required correction: make both persisted rename sequences non-decreasing at
the database boundary, and retain the two adversarial regressions alongside the
existing cycle tests.

## Baseline and control evidence

| Check | Result |
| --- | --- |
| `git diff --check e40b5f42759c06c15ea8d666a18b843d950db51d..527decf58cfcd68ad69f60dad5a5d30a3551ac83` | passed |
| candidate `kontor-store` Jira materialization suite, excluding the two injected probes | passed, 28/28 |
| task and epic sequence-rewind probes | **failed protection**, 0/2 |
| codebase knowledge-graph index | ready at the exact candidate; no partial or skipped parse state reported |

A full workspace suite was not rerun after the release blocker reproduced; no
broader green claim is made by this audit.

## Handoff and audit boundary

The durable task ledger contains no settled fourth verifier turn, fourth gate
evaluation, or locator for a round-four `high-verification-report`. The
chain-of-custody question and the assumption used for this independent audit are
recorded as `OQ-005`; this report does not treat round four as approved.

No Jira, PR, topology, deployment, or production-code state was changed by the
audit seat.

