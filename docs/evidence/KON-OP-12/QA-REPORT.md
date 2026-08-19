# KON-OP-12 / ASMA-7881 — QA report

Verdict: **passed**

Validated at PR #45's exact pushed head
`9ca79f6d9b604ef639aae3d68d858cf6c9203268` (`a883f00` code plus
`9ca79f6` evidence). The checked-out head and `origin/master` merge base were
verified before testing; the base is `615d24bf658c8b00ab94a28664bffeca82b5d3fd`.

## Acceptance validation

- The core disposition suite passed: 27 tests, including all three dispositions,
  reopening, re-disposition after reopening, authority split and detector purity.
- The completion state-machine suite passed: 10 tests. It proves undispositioned
  and reopened questions withhold `MarkDone`, each current disposition releases
  the gate, late questions are considered, and closeout receipts and open
  questions are independent gates.
- The store ledger suite passed: 12 tests. It exercises project isolation,
  stale-revision refusal, current-deferral-only reopening, immutable history,
  restart/export/import/snapshot preservation, and the SQL trigger probe with
  `PRAGMA foreign_keys = ON` plus a positive control.
- `the_operational_hardening_v35_lineage_converges_without_losing_its_receipt`
  passed against a constructed historical v35 database, reaching schema v41 and
  retaining its receipt.

The stale-firing boundary is correct by direct state-machine inspection and the
persisted full-history round trip: `status()` checks firing only against the
current disposition ordinal, and a reopened question subsequently resolved with
a superseding disposition remains `Resolved` while retaining the earlier firing.

## Commands run

All commands exited 0:

```text
cargo test -p kontor-core --test open_question
cargo test -p kontor-scheduler --test completion
cargo test -p kontor-store --test open_questions
cargo test -p kontor-store --test schema_v1 the_operational_hardening_v35_lineage_converges_without_losing_its_receipt -- --exact
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The first workspace attempt was blocked by the execution sandbox refusing a
loopback test bind in `kontor-cli/tests/memory_parity.rs`. Re-running the exact
same command with local-loopback permission passed in full; the loopback test,
the v35 lineage test, and all other workspace tests passed. Expected tests that
require disposable live runtimes/accounts remained explicitly ignored.

No code was modified during QA.
