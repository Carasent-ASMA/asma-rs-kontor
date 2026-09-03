# KON-OP-22 release notes

Date: 2026-09-03

Status: deterministic epic-route correction independently approved and complete
local gate passed; PR, merge, deployment and live receipts pending.

Candidate commits:

- `1ea52d9` — resident deterministic Jira convergence and completion recovery;
- `1793144` — reproducible `Cargo.lock` refresh required by the archive gate.

Those commits were merged through PR #155 as `3d391d0`. This follow-up release
corrects the remaining live ASMA epic-route divergence found during promotion.

## Operator-visible outcome

Kontor now owns deterministic, resident Jira convergence for both confirmed
tasks and confirmed epics. A backlog change no longer depends on a manual sync
command: committed changes wake reconciliation, while a 30-second scan repairs
missed notifications or restarts.

The controller fails closed unless the exact shipped workflow revision required
by the subject is installed in the project. For the ASMA operational project,
promotion therefore includes installing and reading back:

- `connector.jira` / `asma` / `task` / version `2`;
- `connector.jira` / `asma` / `epic` / version `2`.

The existing generic task version `1` remains installed for tasks whose frozen
profile requires it. Historical epic version `1` remains readable and
hash-stable but is not the current bundled revision. Epic version `2` advances
only along this validated route:

`New (10227)` -> `DRAFT (10237)` -> `TO BE GROOMED (10236)` ->
`Groomed (10233)` -> `READY FOR DEVELOPMENT (10213)` ->
`In Development (10214)`.

## Live promotion plan

1. Merge the independently reviewed commit to `master` without GitHub Actions;
   the recorded local gates are the release authority for Kontor.
2. Build `kontor`, `kontor-daemon` and `kontor-mcp` from the exact merge SHA.
3. Back up the serving database, binaries and launch configuration.
4. Replace the fleet, restart `com.asma.kontor.daemon`, and require schema v83,
   a healthy loopback listener and an open reconciliation barrier.
5. Install and read back generic epic workflow revision 2 under a fresh project
   CAS revision; task workflow revision 2 is already installed.
6. Verify `ASMA-8049` traverses each remaining route hop through resident
   observe/apply/refetch, reaches `In Development`, and replays without a
   duplicate effect.
7. Resolve the superseded revision-1 `no_live_transition` conflict only after
   the revision-2 convergence is confirmed.

Rollback is by restoring the pre-deploy database and matching binary fleet as
one unit. A pre-v83 binary cannot serve a v83 database.
