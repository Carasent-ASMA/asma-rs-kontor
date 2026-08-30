# KON-OP-20 — owned per-seat OpenCode config root (installed-binary audit)

Task `01a02a7f-8e47-7682-be52-1b9f2a632ac4` (permission posture). **Not ASMA-7968.**
Audited against the installed OpenCode **1.18.15** and Paseo **0.6.1** on
2026-08-30. User and global configuration were read only and left byte-identical.

## Why the earlier designs failed

1. **Reimplementing config resolution** (merging global → root → `.jsonc`
   siblings in Rust) is unsound: the deciding inputs include environment
   variables read by the *spawned* process.
2. **Three environment variables**
   (`OPENCODE_PERMISSION`, `OPENCODE_CONFIG_CONTENT`,
   `OPENCODE_DISABLE_PROJECT_CONFIG`) are **not** sufficient. They *merge* over
   ambient configuration rather than replacing it — measured, not assumed — so a
   late-sorting nested rule such as `bash: {"*git*": "allow"}` survives and, under
   `findLast`, wins over the floor. `OPENCODE_CONFIG` and `OPENCODE_CONFIG_DIR`
   merge the ambient global too rather than replacing it.

## What was verified to work

A **Kontor-owned per-seat configuration root**, with all six of:

| Variable | Value |
| --- | --- |
| `XDG_CONFIG_HOME` | the owned per-seat root |
| `OPENCODE_CONFIG` | the owned `opencode.json` |
| `OPENCODE_CONFIG_DIR` | the owned `opencode` directory |
| `OPENCODE_CONFIG_CONTENT` | the exact full controlled config, canonical JSON |
| `OPENCODE_PERMISSION` | the exact permission object, canonical JSON |
| `OPENCODE_DISABLE_PROJECT_CONFIG` | `true` |

`XDG_DATA_HOME` and `XDG_STATE_HOME` are **not** overridden: provider
authentication lives there and a seat that cannot authenticate is not a seat.

### Measured result

Hostile inputs used: this host's real global (`edit: allow`, `task: allow`,
`bash {"*": "allow"}`), a repository `opencode.json` (`bash {"*git*": "allow"}`,
`task: allow`), an `opencode.jsonc` sibling (`browser: allow`), and a seat-local
`.opencode/opencode.json` (`bash {"*git*": "allow"}`).

- **Without** the owned root, `opencode debug config --pure` resolved
  `*git*: allow`, `browser: allow`, `edit: allow` and `task: allow` — every
  hostile value survived.
- **With** the owned root and the six variables, the resolved `permission` was
  **byte-identical** to the rendered block: `*git*` gone, `browser` gone,
  `edit: deny`, `task: deny`, `external_directory: {"*": "deny"}`, and the
  five-member destructive floor intact.

## Consequences for the design

- The owned root is **per seat**, which also removes the shared-worktree race:
  two seats in one worktree get two roots and cannot overwrite one another.
- The worktree-local `.opencode/opencode.json` composition becomes unnecessary
  for posture, and with `OPENCODE_DISABLE_PROJECT_CONFIG=true` it is not read at
  all — which also retires the tracked/foreign-file ownership problem.
- The owned config must carry the **complete** configuration the seat needs to
  launch, including its MCP surface, because it replaces the operator's root.
- Fail-closed remains the fallback for a Paseo without per-agent `--env`, not the
  permanent OpenCode outcome.

## Paseo surface

`paseo agent run --help` on 0.6.1 documents
`--env <key=value>  Set environment variable(s) for the agent process (can be
used multiple times)`. This is distinct from `PaseoCommand::env`, which sets the
environment of the CLI invocation itself and already carries
`KONTOR_CALLER_AGENT_ID`.
