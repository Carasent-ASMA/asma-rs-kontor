# ASMA-8120 open-question ledger

Date: 2026-09-19
Task: Jira `ASMA-8120` / Kontor `01a07722-c3ed-7a63-94e6-cefd22e438ab`

This ledger records unresolved state before any rollout choice depends on it.
An open entry is a stop condition for its affected operation, not permission to
pick one of the listed options.

## OQ-8120-01 — post-deploy attachment convergence

- **Subject:** whether the deployed exact-binding rehydration fix has converged
  for the running ASMA-8120 scope seat.
- **Attaches to:** deployment bundle
  `ASMA-8120-20260919T205522Z-95bcf86`, merged PR #243, TeamRun
  `01a0bb61-b8e0-7bf0-a14f-b44713ca10d7`, scope AgentRun
  `01a0bb61-b8e0-7bf0-a14f-b450a46dafdc`, and the ASMA-8120 high-scope
  record.
- **Why the state is ambiguous:** the current Kontor projection reports the
  scope AgentRun attached and running, and a fresh ASMA-8049 native-name
  preview read both current seats. The daemon startup log nevertheless records
  a post-barrier attempt at `2026-09-19T20:56:38Z` where Paseo refused the
  scope workspace because it was not the exact container the runtime had
  prepared. The canonical runtime timeline was unavailable (`503`), so the
  ordering and finality of those observations cannot be proven.
- **Options seen:** (a) runtime history proves the warning was a stale retry
  after a successful exact attachment, so no correction is needed; (b) it is a
  remaining idempotent-replay defect requiring one root-cause fix and exact
  regression test; (c) deployment convergence failed, so rollout stops and the
  retained binary/definition rollback route is used.
- **Disposition:** **OPEN.** Implementation must obtain exact runtime history
  or another authoritative ordered receipt and close this entry before it
  claims deployment convergence or publishes/selects successor definitions.

## OQ-8120-02 — active epics without confirmed migration inputs

- **Subject:** completion treatment of four current active epics that do not
  have the confirmed Jira binding and Team Definition pin required by the
  supported upgrade route.
- **Attaches to:** TASK-005 acceptance text, the architecture section
  “Migration scope and failure handling,” and the baseline fleet census in the
  ASMA-8120 high-scope record.
- **Why the state is ambiguous:** the task says to migrate all remaining
  current epics, while the governing architecture limits Jira-key rendering to
  Jira-bound items and says a missing confirmation blocks the affected item.
  The live census contains four active legacy epics in that state: QNR v2
  Nonprod Delivery (no pin/binding), Kontor Operator Surface and Attention
  Channel (no pin/binding), Catalog workspace without gitlinks (no
  pin/binding), and Repair stale-native Core Team seat succession (Operational
  v3 pin, no Jira binding).
- **Options seen:** (a) first repair each binding/pin through a separately
  authorized supported workflow, then include it; (b) fence the four entries
  and define this rollout complete over the thirteen currently eligible
  confirmed/pinned epics; (c) classify individual entries as historical and
  explicitly remove them from “current” scope.
- **Disposition:** **OPEN.** Do not infer keys, create replacement topology, or
  migrate these four epics. The implementation handoff may proceed only for
  read-only preview work until an authoritative owner resolves whether
  ASMA-8120 completion waits for option (a), (b), or (c).
