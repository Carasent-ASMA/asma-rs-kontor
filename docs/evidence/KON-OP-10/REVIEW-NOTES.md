# KON-OP-10 code-review-gate — review notes

Verdict: **passed** (inspector recovery, ASMA-7879).

Reviewed at pushed head `4a0ef58ac6e4055106e263b3f6de5e4d736c931f` on PR
[#87](https://github.com/Carasent-ASMA/asma-rs-kontor/pull/87).

Judged against OP-REQ-032 / KON-OP-10 in
`_docs/ai-orchestration/plans/2026-08-14-23-21-plan-kontor-operational-mvp.md`,
and against the recovery architect's pinned "smallest isolated admission
narrative" scope.

## What was reviewed

One new scheduler integration test
`the_operational_seven_run_ceiling_admits_four_through_seven_and_refuses_the_eighth`
in `crates/kontor-scheduler/tests/ready_batch.rs`, plus
`docs/evidence/KON-OP-10/CODE-CHANGE.md` and `QA-REPORT.md`.

No production scheduler, daemon, or QNR code changed. That matches the ticket
Owns list: proof and evidence, no QNR production changes unless the owner
opts in.

## Findings

1. **The new test proves the pass arithmetic, not the two-clean fold.**
   `AdaptiveWindow::observe` grows on every `Clean`. The production
   two-distinct-clean gate lives in `kontor_accounts::fold`. The code-change
   names that split; this is not a defect of the test as written. EVD-OP-012's
   two-observation clause is carried by the existing accounts tests, not by
   this new case.

2. **Live 14-step isolated-home / disposable Jira / QNR pilot is not in this
   PR.** The architect recorded that honestly. Completing KON-OP-10 on this
   PR accepts the deterministic core and leaves the live fleet narrative as
   an explicit remainder for OP-11, not as silent scope reduction.

3. **No hidden production change.** Diff is +233/−1 across the test and two
   evidence files.

## Verdict

**PASS for code-review-gate** within the recovery-pinned scope (integrated
4→5→6→7 + eighth `capacity_exhausted` naming `Capacity { limit: Mission,
remaining: 0 }`). Do not treat this PR as the live EVD-OP-010/012 fleet
receipt.
