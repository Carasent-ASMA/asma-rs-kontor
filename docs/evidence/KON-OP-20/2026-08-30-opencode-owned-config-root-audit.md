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

---

## Delivered path (2026-08-30)

OpenCode delivery is reachable again, and only behind the proof below. The
earlier fail-closed refusal remains as the fallback for a Paseo that cannot
carry per-agent environment.

### Order of operations, before anything native exists

1. Daemon capability — `supports_seat_environment()`, pinned at Paseo `0.6.1`,
   read from the **daemon's** reported version. `paseo agent run --help` on 0.6.1
   documents `--env <key=value>`, repeatable. This is distinct from
   `PaseoCommand::env`, which sets the CLI invocation's own environment.
2. Binary identity — `paseo provider diagnostic <provider> --json` reports
   `Resolved path: /opt/homebrew/bin/opencode` and `Version: 1.18.15`. Only a
   version in the proved set is admitted.
3. Owned root materialized under the realm state root: one plain path component
   per seat, symlinked components refused, `0700`/`0600`, read back and hashed.
4. Preflight — that binary, that working directory, that environment; the
   complete resolved permission object must equal the renderer.
5. `agent run --env` carries the identical six variables, and the posture digest
   travels in the labels the recovery census matches on.

### Corrections this pass made to earlier claims

- Three variables were **not** enough. The permission carriers merge rather than
  replace, so a nested `bash: {"*git*": "allow"}` survived them.
- The six keys do **not** erase every ambient layer by construction either: the
  load order places an auth-backed active-org config and a system managed layer
  *after* `OPENCODE_CONFIG_CONTENT`. Full-object comparison is the guarantee;
  `managed_configuration_survives_and_is_caught_by_full_comparison` proves that
  detection is load-bearing rather than decorative.
- `OPENCODE_PURE` disables external plugins only and is not containment.
- `XDG_CONFIG_HOME` and `OPENCODE_PERMISSION` are redundant with
  `OPENCODE_CONFIG_DIR` and `OPENCODE_CONFIG_CONTENT` on the host measured, in
  that dropping either changed no resolved value. They are retained — which layer
  wins is the installed build's choice — and the boundary suite pins the set so a
  silent removal fails.

### Mutation proof (10/10 killed)

Dropping each of the six variables; a block that stops naming the effectful
tools; a preflight that compares only `bash`, skips the comparison, or accepts
any version; an adapter without the capability gate; and an argv validator that
accepts a partial set.

### Retired with this change

The worktree composer for OpenCode is deleted, not merely bypassed: with project
configuration disabled the file would not be read, writing it would reintroduce
the shared-worktree race, and a reader with no caller is history waiting to be
mistaken for authority. Claude composition is untouched, and a launch-boundary
test asserts an OpenCode launch leaves the worktree and its git excludes alone.

The post-placement re-read went with it. The environment is carried on the create
call itself and the owned root sits outside anything the seat can write, so there
is no window between the proof and the spawn for a compensating archive to cover.

### Known limits, stated rather than claimed away

- **Equality is narrow.** Binary identity and version, working directory, the
  six-key environment and the owned files by hash. `HOME` and the data and state
  roots are *inherited*, not asserted equal; `prove_preserved_roots` bridges that
  by checking the data root holds the credentials the diagnostic names.
- **`paseo agent update` exposes no mode or environment update**, so a posture is
  fixed at spawn and a plan-mode reserve cannot be promoted through Kontor's CLI
  port.
- **No live authenticating seat has been launched** through this path yet.
