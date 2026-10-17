# Kontor Paseo plugin

The Kontor-owned Paseo plugin. This is the KR-11 delivery of the Kontor
relaunch plan (ASMA-8507): the decided binding hooks plus the
`before("workspace.create")` placement guard.

## Hooks

| Hook | Behaviour |
| --- | --- |
| `before("agent.create")` | Adds one stable system-prompt line and the `kontor` MCP server at **observer** authority. A server already named `kontor` is preserved, so Kontor-composed seats are untouched. |
| `before("agent.session_open")` | Reads the session's own labels from the local daemon and sets `KONTOR_SEAT` and `KONTOR_BINDING_GENERATION`, or `KONTOR_SEAT=unbound`. A failed or slow read sets `KONTOR_SEAT=unresolved`; it never blocks the open. |
| `before("workspace.create")` | Refuses a labelled Kontor seat's explicit workspace creation outside Kontor placement. Interactive and unlabelled creation is unchanged, and an unreadable label lookup fails open. |

The plugin adds no UI, no menu and no agent-creation path.

## Kontor placement

A labelled seat may register an existing directory under an explicit Paseo
project. Everything else is refused as outside Kontor placement:

- a Paseo-owned worktree source (`kind: "worktree"`);
- a directory without an explicit `projectId` (Paseo would infer a project);
- a relative directory path;
- a directory inside Paseo's own `~/.paseo/worktrees` root;
- a directory outside `KONTOR_PLUGIN_PLACEMENT_ROOTS`, when configured.

This is the shape check only. Exact project, canonical cwd and native readback
stay with `kontord`; the plugin holds no Kontor write credential.

## Authority

The composed `kontor-mcp` server always runs at `--credential-tier observer`.
The tier is not configurable. The plugin names the realm state root but never
reads `credentials.json`, never receives `KONTOR_AUTH`, and never writes to
`kontord`. `test/security.test.ts` pins that boundary.

## Install and uninstall

Install from this repository path; no registry is involved:

```bash
paseo plugin install /path/to/asma-rs-kontor/apps/paseo-plugin --json
paseo plugin ls --json
```

Uninstall stops the plugin and returns Paseo to its previous behaviour:

```bash
paseo plugin remove kontor --json
paseo plugin ls --json
```

The isolated closure suite never touches the live daemon or `~/.paseo`:

```bash
apps/paseo-plugin/scripts/closure-suite.sh
```

It starts a disposable daemon home on a free loopback port, installs the plugin
from this directory, proves the guard and the interactive path, removes the
plugin, and proves the refused create succeeds again.

## Configuration

All values are optional and read from the daemon environment:

| Variable | Default | Meaning |
| --- | --- | --- |
| `KONTOR_PLUGIN_STATE_ROOT` | `~/.local/state/kontor/asma` | State root argument passed to `kontor-mcp`. |
| `KONTOR_PLUGIN_MCP_COMMAND` | `kontor-mcp` | Command used for the composed server. |
| `KONTOR_PLUGIN_MCP` | on | `off` skips the MCP server; the hooks and guard stay active. |
| `KONTOR_PLUGIN_SERVE_PROFILE` | none | Optional `--serve-profile` narrowing. |
| `KONTOR_PLUGIN_PLACEMENT_ROOTS` | none | Comma-separated extra containment roots. |
| `KONTOR_PLUGIN_LABEL_TIMEOUT_MS` | `2000` | Label-read budget before failing open. |

## Tests

```bash
pnpm --filter @carasent/kontor-paseo-plugin typecheck
pnpm --filter @carasent/kontor-paseo-plugin test
apps/paseo-plugin/scripts/closure-suite.sh
```

## Known limit — Paseo 0.11.2

The guard covers an explicit `workspace.create` request whose initial agent
carries Kontor labels, and any caller identity a future release passes in the
hook request. Paseo 0.11.2 does not pass caller identity, and its Paseo MCP
`create_workspace` tool does not run the `workspace.create` hook at all:

- `WorkspaceCreateRequestSchema` in `@getpaseo/protocol` 0.11.2 has no
  top-level `callerAgentId`; the lifecycle request schema is `.strict()`, so
  the daemon cannot deliver one to the hook.
- `create_workspace` in the Paseo tool host calls the provisioning service
  directly (`packages/server/src/server/agent/tools/paseo-tools.ts` and
  `bootstrap.ts`), so the plugin never runs for that path.

Until Paseo supplies caller identity to the hook or routes the MCP tool through
it, a labelled seat can still bypass the hook with the MCP `create_workspace`
tool. That residual is recorded in the ASMA-8507 evidence with the upstream
action; G-20 is not closed by the hook alone.
