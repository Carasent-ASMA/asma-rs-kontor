# ASMA-8507 — Kontor Paseo plugin floor and workspace guard

> **Date:** 2026-10-18
> **Base:** `0971674d` (`origin/master`), rerun at `4d91d258` plus the round-2 fixes
> **Paseo:** 0.11.2
> **Scope:** KR-11 of the Kontor relaunch plan. Plugin source in
> `apps/paseo-plugin`.

## Delivered

| File | Purpose |
| --- | --- |
| `apps/paseo-plugin/paseo-plugin.json` | Plugin manifest, id `kontor`, Paseo `>=0.11.0`. |
| `apps/paseo-plugin/index.server.ts` | Server entry point. |
| `apps/paseo-plugin/server/hooks.ts` | The three before hooks and the guard. |
| `apps/paseo-plugin/server/config.ts` | Read-only configuration; observer tier is fixed. |
| `apps/paseo-plugin/server/labels.ts` | Kontor/Paseo-direct seat detection and seat generation. |
| `apps/paseo-plugin/server/agent-labels.ts` | Bounded local label read; every failure returns `null`. |
| `apps/paseo-plugin/server/placement.ts` | Kontor placement check against the seat's bound project and canonical worktree. |
| `apps/paseo-plugin/test/*.test.ts` | Unit tests, including the no-write-credential boundary. |
| `apps/paseo-plugin/scripts/closure.mjs` | Assertions against a running daemon. |
| `apps/paseo-plugin/scripts/closure-suite.sh` | Isolated install → guarded → uninstall → restored run. |

The hooks are the decided ones: `agent.create` adds the Kontor MCP at
observer authority and one stable context line; `agent.session_open` sets
`KONTOR_SEAT`/`KONTOR_BINDING_GENERATION`, `unbound` or `unresolved`; the
`workspace.create` hook refuses a labelled seat outside Kontor placement.

## Unit and type gates

```
$ pnpm --filter @carasent/kontor-paseo-plugin typecheck
$ tsc --noEmit
typecheck exit=0

$ pnpm --filter @carasent/kontor-paseo-plugin test
Test Files  5 passed (5)
     Tests  47 passed (47)
```

## Isolated closure run

`apps/paseo-plugin/scripts/closure-suite.sh` creates a disposable Paseo home
under the system temp directory, a free loopback port and its own git fixture.
It starts the daemon with `KONTOR_PLUGIN_MCP=off`, so the plugin never reaches
the live state root or the live realm. It does not read or write `~/.paseo`,
does not restart the live daemon and does not touch the running PR-desk
agents.

Install readback:

```json
{"id":"kontor","status":"running","identity":{"kind":"directory","path":".../apps/paseo-plugin"}}
```

Guarded behaviour:

```json
{"expected":"guarded","checks":[
 {"name":"interactive directory create","outcome":"created",
  "workspaceId":"wks_4c3a86558c3c35d2"},
 {"name":"labelled worktree create","outcome":"refused",
  "message":"Plugin kontor before workspace.create failed: Kontor workspace guard refused: Kontor placement registers an existing directory; worktree creation is outside it"},
 {"name":"labelled directory without projectId","outcome":"refused",
  "message":"Plugin kontor before workspace.create failed: Kontor workspace guard refused: Kontor placement requires an explicit projectId"},
 {"name":"interactive worktree create","outcome":"created",
  "workspaceId":"wks_38b6bced692f1c1c"}]}
```

CLI residual with the same installed plugin — `paseo workspace create` carries
no agent and no caller, so the guard cannot see a seat:

```json
{"name":"cli workspace create","outcome":"created","workspaceId":"wks_dfbf84b672dcefb0"}
```

Uninstall readback and restored behaviour:

```json
{"plugins":0}
{"expected":"unguarded","checks":[
 {"name":"interactive directory create","outcome":"created",
  "workspaceId":"wks_6556b4c15ee3d275"},
 {"name":"labelled worktree create","outcome":"created",
  "workspaceId":"wks_85af55ac9f25ce21"},
 {"name":"interactive worktree create","outcome":"created",
  "workspaceId":"wks_16598d78f9a816cb"}]}
```

The same labelled worktree create that the installed plugin refused succeeds
after `paseo plugin remove kontor`; the uninstall restores the previous
behaviour.

The suite was run from this fix checkout with `GIT_CONFIG_COUNT=0` so the
desk's global `core.hooksPath` does not reach the disposable fixture repo; the
daemon, the plugin install and the assertions are unchanged.

## No write credential

- The composed MCP server is always `--credential-tier observer`; the tier is
  not configurable (`server/config.ts`, `DEFAULT_CREDENTIAL_TIER`).
- Production plugin code contains no `credentials.json`, `KONTOR_AUTH`,
  `admin`/`operator` tier, `daemon-keypair`, HTTP write or child-process call.
  `test/security.test.ts` fails if one is introduced.
- The plugin names the realm state root but never opens it.

## MCP server name — no collision with a provider home

The plugin composes its server as `kontor-observer`, never `kontor`, so a
home-level `kontor` server at any tier is untouched by whatever merge rule a
provider applies.

Source probe of the installed Paseo 0.11.2 bundle:

- **Codex** — `buildCodexInnerConfig` in
  `dist/server/server/agent/providers/codex-app-server-agent.js` builds
  `innerConfig.mcp_servers` from the request's `mcpServers` and passes it as a
  config override; a request entry named `kontor` would replace the provider
  home's `[mcp_servers.kontor]` as a whole. This is the collision the rename
  removes.
- **Claude** — `claude/agent.js` passes the request `mcpServers` as explicit
  SDK options, which take precedence over a same-named settings entry.
- **opencode** — `opencode-agent.js` registers each request server through
  `client.mcp.add({ name, config })`; which entry wins for a name that already
  exists in the provider config is not recorded here. Open, and not needed for
  this plugin because the name differs.

The unit test "does not replace a Kontor server already composed for the seat"
pins the request-level half: a request `kontor` entry is preserved and the
plugin adds `kontor-observer` beside it.

## Residual blocker — Paseo 0.11.2 workspace creation paths

The guard sees a caller only when the request carries one. Paseo 0.11.2
carries the caller as `agent.callerAgentId` on `workspace.create` and passes
it to the hook (`WorkspaceInitialAgentSchema` keeps the field from
`AgentCreateRequestSchema`), and the guard reads it. Three paths still create
a workspace with no seat and no caller:

- The Paseo tool host's `create_workspace` calls
  `workspaceProvisioning.createWorkspaceForDirectory` directly
  (`packages/server/src/server/agent/tools/paseo-tools.ts` and
  `bootstrap.ts`), without `pluginRuntime`, so the hook never runs.
- `paseo run --new-workspace` creates the workspace before the agent exists;
  the `workspace.create` request has no agent and no caller.
- `paseo workspace create` creates a workspace with no agent and no caller.

The guard therefore enforces on the labels the hook does see: the initial
agent's `agent.labels` and `agent.callerAgentId` on an explicit
`workspace.create` request. A labelled seat can still bypass the hook with the
MCP `create_workspace` tool or either CLI path. G-20 is not fully closed by
this hook until Paseo supplies a caller on every workspace creation and routes
the MCP tool through the hook.

- **Owner:** Kontor LSA for the upstream ask; Paseo upstream for the change.
- **Next action:** widen the upstream ask: pass the caller on every
  `workspace.create` request, including MCP `create_workspace` and the CLI
  (`paseo run --new-workspace`, `paseo workspace create`), or route those
  paths through the hook.
- **Resumption trigger:** a Paseo release that carries caller identity on all
  three paths, or routes them through the hook; re-run
  `apps/paseo-plugin/scripts/closure-suite.sh` with a caller-labelled seat,
  including the CLI case.
- **Interim:** keep `create_workspace` out of ECP/seat tool surfaces per the
  QNR-P1-06 containment note.
