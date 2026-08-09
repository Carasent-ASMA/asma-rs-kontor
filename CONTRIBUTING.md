# Contributing to Kontor

The Kontor MVP epic is planned in
`asma-modules/_docs/ai-orchestration/plans/2026-08-08-20-12-plan-asma-kontor-mvp-control-plane.md`
and executed through Jira tickets (`KON-MVP-01` … `KON-MVP-22`) against the
AgentsRoom tracker. Until the epic is closed:

- The root `Cargo.toml` member list and `[workspace.dependencies]` pins are
  owned by KON-MVP-02 (CON-007). Do not edit them for a ticket; return to the
  tracker for re-planning.
- One ticket is one reviewable diff; code tickets run in worktrees that
  initialize only their declared `mod:` submodules (CON-005, CON-006).
- Every runtime-changing ticket includes unit/contract tests and a mutation
  check for its changed behavior (CON-004).
- No test may bind a socket, launch the daemon or Tauri event loop, fetch
  network data, or leave a child process behind (TST-001).
- Never stage or commit another agent's uncommitted work; stage explicit paths
  only.

## Pull requests

- Run the full gate set from `README.md` before requesting review.
- Keep the generated `Cargo.lock` and `pnpm-lock.yaml` in sync with your
  change.
- The `scripts/verify-tree.py --mode archive` preflight must pass after your
  branch is committed.

## Licensing

Kontor source is `MIT OR Apache-2.0`. By contributing you agree to license
your contribution under both, with copyright assigned per `NOTICE`.
