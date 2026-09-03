# KON-OP-22 release notes

Date: 2026-09-03

Status: candidate independently approved and committed-tree verified; PR,
merge, deployment and live receipts pending.

Candidate commits:

- `1ea52d9` — resident deterministic Jira convergence and completion recovery;
- `1793144` — reproducible `Cargo.lock` refresh required by the archive gate.

## Operator-visible outcome

Kontor now owns deterministic, resident Jira convergence for both confirmed
tasks and confirmed epics. A backlog change no longer depends on a manual sync
command: committed changes wake reconciliation, while a 30-second scan repairs
missed notifications or restarts.

The controller fails closed unless the exact shipped workflow revision required
by the subject is installed in the project. For the ASMA operational project,
promotion therefore includes installing and reading back:

- `connector.jira` / `asma` / `task` / version `2`;
- `connector.jira` / `asma` / `epic` / version `1`.

The existing generic task version `1` remains installed for tasks whose frozen
profile requires it.

## Live promotion plan

1. Merge the independently reviewed commit to `master` without GitHub Actions;
   the recorded local gates are the release authority for Kontor.
2. Build `kontor`, `kontor-daemon` and `kontor-mcp` from the exact merge SHA.
3. Back up the serving database, binaries and launch configuration.
4. Replace the fleet, restart `com.asma.kontor.daemon`, and require schema v83,
   a healthy loopback listener and an open reconciliation barrier.
5. Install the exact task and epic workflow revisions under fresh project CAS
   revisions and read them back as installed.
6. Verify the confirmed ASMA epic and tasks converge or expose typed conflicts;
   do not infer Jira identity from a rendered item code.

Rollback is by restoring the pre-deploy database and matching binary fleet as
one unit. A pre-v83 binary cannot serve a v83 database.
