# KON-OP-22 release notes

Date: 2026-09-03

Status: merged, deployed and verified live.

Candidate commits:

- `1ea52d9` — resident deterministic Jira convergence and completion recovery;
- `1793144` — reproducible `Cargo.lock` refresh required by the archive gate.

Those commits were merged through PR #155 as `3d391d0`. This follow-up release
corrects the remaining live ASMA epic-route divergence found during promotion.
The correction commit `1951cac` was merged through PR #156 as `7c27f4d`.

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

## Live promotion receipt

1. PR #156 merged to `master` as
   `7c27f4d7a8e2aa37c1b1ddc576fe60387e95cf47`; there were no GitHub Actions,
   by the explicit Kontor local-gate policy.
2. The complete clean-archive verifier passed, and all three release binaries
   were built from that exact merge.
3. The coherent rollback unit is
   `/Users/igor/.local/state/kontor/asma/deploy-backups/20260903T143337Z-kon-op-22-7c27f4d/`.
   It includes the previous fleet, LaunchAgent, runtime configuration, and
   verified snapshot
   `kontor-01a00649-9ee6-73e0-ba1b-6a6c35cfd065-20260903T143417410639Z.db`.
4. The fleet was atomically replaced and restarted. PID `18681` serves the
   healthy loopback realm on schema v83 with clean integrity and foreign keys.
5. Epic workflow revision 2 installed and read back at project revision 5 with
   receipt `01a067cf-beda-72c2-ac30-6042125a1f89`.
6. The resident controller confirmed these exact `ASMA-8049` destinations:
   `TO BE GROOMED (10236)`, `Groomed (10233)`,
   `READY FOR DEVELOPMENT (10213)`, and `In Development (10214)`.
7. The post-backstop ledger remained exactly four confirmed revision-2 intents
   and zero open conflicts. The superseded revision-1 conflict was then closed
   by receipt `01a067d0-6c17-7d02-a46d-602f57b1e5f3`.
8. `ASMA-8050` and `ASMA-8062` both read back as converged with empty diffs.

Rollback is by restoring the pre-deploy database and matching binary fleet as
one unit. A pre-v83 binary cannot serve a v83 database.

## Pending post-release recovery correction

Closeout exposed one further fail-closed recovery case: the exact ASMA-7869
materialization is split across two legacy pending batches. The correction
recovers identical mixed Link/Create requests and accepts several legacy
fragments only when they form one exact, non-overlapping ordinal cover. It
retains original batches and item ownership, records the exact recovery set in
the immutable ledger and proves replay creates no duplicate external effect.
Promotion must replay the already persisted Kontor command; direct Jira and
database writes remain prohibited.

The first promotion of that correction (PR #158, merge `2b544ac`) exposed one
narrow proof-scope defect without committing a Jira effect: ordinary historical
`Link` items were incorrectly required to carry a Kontor creation marker. The
follow-up scopes marker proof to recovered `Create` items only; exact linked
identity, project, issue type and parent remain mandatory for every `Link`.

PR #159 merged that proof-scope correction as `eba40aa` and deployed daemon
hash `73e36142d743e965bf7dc58a239837c77b061877dcbfaef564c305fa263e700a`.
The persisted replay then confirmed `ASMA-7869` and exposed the final legacy
compatibility error before creating any new Jira issue: Kontor treated its
internal task hierarchy role as a requirement for Jira's literal `Task` type.
All 18 existing children are correctly parented hierarchy-level-zero Jira work
items, but their project-defined types are 16 `User Story` and two `Tech tasks`.
Ordinary explicit links now accept that standard Jira hierarchy role without
claiming the project-specific type name. Creates and recovered creates remain
literal-`Task`, marker, content and parent strict.
