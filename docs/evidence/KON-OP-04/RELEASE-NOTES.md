# KON-OP-04 Release Notes

> **Date:** 2026-08-17 17:42 CEST
> **Status:** 🟢 Approved
> **Category:** report
> **Scope:** ASMA-7873 / KON-OP-04 at submodule HEAD `82dc375`
> **Summary:** Release confirmation for the durable Core Team, Quick-session and Quick-to-epic promotion surfaces, including recovery behavior introduced by the review remediation.

---

## When to Load

**Load this document when:**

- releasing or integrating KON-OP-04;
- checking which Operational successor routes became available;
- reviewing the accepted residuals around promotion handoff and PSW readback.

**Do NOT load for:** OP-05+ Advisor, Committee, completion, Jira or diagnostic-UI work except where it consumes these APIs.

---

## What ships

- Immutable Project Core Team read, preview and apply with exact catalog-role
  resolution, mandatory distinct `LSA`/`TPM` normalization, closed seat policy,
  stable replay and explicit epic-seat materialization.
- Derived Quick-role projection and durable Quick-session ensure with eligibility
  refusal, stable session/node/seat identities and lost-ack replay.
- Tracker-neutral Quick-session promotion preview/apply that freezes one epic,
  ESW, ECP and roster; materializes required/default seats; leaves on-demand
  seats absent; delivers immutable source evidence to the frozen LSA; and
  returns the original result on replay.
- Explicit epic-roster upgrade preview/apply that preserves frozen epic state
  until requested and adds only newly required/default seats.
- Recovery guarantees: promotion authorization and roster persistence are one
  transaction, while an interrupted Quick-session ensure reconciles the exact
  persisted node and seat ids.

## Verification

Release confirmation on `82dc375` executed 15 distinct focused checks: 12
public loopback API tests and 3 store atomicity tests. All passed with 0
failures. They cover Core Team preview/apply/read/materialize and refusal rules,
Quick-session eligibility/idempotency, promotion/materialization/replay, frozen
roster upgrade, interrupted-promotion resume, interrupted-ensure reconciliation,
and promotion/roster rollback.

The preceding QA gate on the same implementation reported 110 suites: 1,394
tests passed, 0 failed and 8 ignored. Workspace Clippy with warnings denied and
Rust formatting checks also passed. See [QA.md](QA.md).

## Known residuals

- Promotion persists a typed promotion-specific handoff document rather than a
  `HandoffCapsule`. Quick sessions intentionally have no `TeamRun`/`AgentRun`,
  while `HandoffCapsule` requires that identity. The shipped representation
  still freezes exact bytes/hash and targets the frozen LSA seat.
- A first placement cannot compare the adopted PSW with a prior native readback
  because none exists yet. A missing adopted base refuses, the first observed
  native id is recorded, and later mismatches refuse without creating a
  fallback project.
- Jira create/link and ASMA activation remain OP-07 scope; Advisors/Committees,
  Completion and diagnostic UI remain OP-05/06/09 scope.

## Verdict

Release confirmation passed. The OP-04 deliverable is ready for the
`release-gate` with evidence `release-notes`.
