# KON-OP-22 independent review notes

Date: 2026-09-03

Verdict: **APPROVE** for PR and merge. No remaining P0/P1 release blocker was
found in the complete current-master reconstruction.

## Reviewed invariants

- Jira synchronization is resident for confirmed task and epic subjects, with
  append-driven wake-up and a bounded 30-second recovery scan.
- Exact entity/profile workflow selection and installed immutable revision
  matching fail closed.
- Canonical Jira identity migration fails closed on ambiguity and preserves one
  active Jira identity while allowing unrelated connector links to coexist.
- Completion create/advance, derived profile publication or exact reuse, wakes
  and command receipt commit atomically.
- Reopened work is detected at startup and during the resident paginated scan;
  remediation evidence remains generation-scoped and immutable.
- API, OpenAPI and MCP operations and their Observer/Operator authority agree.
- No credential material is present in the implementation or its evidence.

## Findings resolved during review

1. Stored completion-profile JSON text was compared directly with an in-memory
   JSON value. The comparison now uses the exact serialization used by insert,
   and two separate epics prove reuse of the same derived profile revision.
2. Task reconciliation initially treated unrelated connector links as Jira
   ambiguity. It now reconciles the exact Jira subset when present. A task with
   only an unsupported connector still receives the existing typed refusal.

## Promotion conditions

- Back up the live schema-v80 database, all three serving binaries and the
  LaunchAgent configuration as one coherent rollback unit.
- Build and deploy `kontor`, `kontor-daemon` and `kontor-mcp` from the exact
  merge SHA, then require schema v83 and a clean foreign-key check.
- Install and read back `connector.jira/asma/task@2` and
  `connector.jira/asma/epic@1` using fresh project revisions.
- Prove confirmed Jira subjects `ASMA-8050`, `ASMA-8062` and epic `ASMA-8049`
  by observe/apply/refetch and replay without duplicate effects.
- Run the archive verifier against the committed tree. GitHub Actions remain
  intentionally disabled; local release gates are authoritative.
