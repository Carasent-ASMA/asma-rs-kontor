# ASMA-8507 — Kontor Paseo plugin floor and workspace guard

> **Date:** 2026-10-18
> **Base:** `0971674d` (`origin/master`), rerun at `056771d0` plus the round-3
> fixes
> **Paseo:** 0.11.4 for the final closure rerun; the source facts below are
> from 0.11.2 and the same paths remain in the upgraded daemon.
> **Scope:** KR-11 of the Kontor relaunch plan. Plugin source in
> `apps/paseo-plugin`.

## Delivered

| File | Purpose |
| --- | --- |
| `apps/paseo-plugin/paseo-plugin.json` | Plugin manifest, id `kontor`, Paseo `>=0.11.0`. |
| `apps/paseo-plugin/index.server.ts` | Server entry point. |
| `apps/paseo-plugin/server/hooks.ts` | The three before hooks and the guard. |
| `apps/paseo-plugin/server/config.ts` | Read-only configuration; observer tier is fixed. |
| `apps/paseo-plugin/server/labels.ts` | Kontor/Paseo-direct seat detection and seat identity. |
| `apps/paseo-plugin/server/agent-labels.ts` | Bounded local label read; every failure returns `null`. |
| `apps/paseo-plugin/server/placement.ts` | Kontor placement check against the seat's bound project and canonical worktree. |
| `apps/paseo-plugin/server/placement-binding.ts` | Read-only binding resolution from `kontor.workspace_id`/`kontor.worktree`. |
| `apps/paseo-plugin/test/*.test.ts` | Unit tests, including the no-write-credential boundary. |
| `apps/paseo-plugin/scripts/closure.mjs` | Assertions against a running daemon. |
| `apps/paseo-plugin/scripts/cli-seat.mjs` | Creates a labelled seat for the CLI bypass cases. |
| `apps/paseo-plugin/scripts/closure-suite.sh` | Isolated install → guarded → CLI cases → uninstall → restored run. |

The hooks are the decided ones: `agent.create` adds the `kontor-observer` MCP
at observer authority and one stable context line only when the request has no
`kontor` server of its own; `agent.session_open` sets `KONTOR_SEAT`, `unbound`
or `unresolved`; the `workspace.create` hook refuses a labelled seat outside
its Kontor placement.

The seat identity reads the labels the Kontor writer actually plants in
`crates/kontor-runtime-paseo/src/wire.rs`: `kontor.seat_binding_id` (hosted
seat), `kontor.role_slot_id` (delivery), and `kontor.agent-run` (run fallback).
No writer plants `kontor.seat` or a binding-generation label, so the plugin
publishes only `KONTOR_SEAT`; a generation key returns only when a writer
carries one.

## Unit and type gates

```
$ pnpm --filter @carasent/kontor-paseo-plugin typecheck
$ tsc --noEmit
typecheck exit=0

$ pnpm --filter @carasent/kontor-paseo-plugin test
Test Files  5 passed (5)
     Tests  49 passed (49)
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
 {"name":"interactive directory create","outcome":"created"},
 {"name":"labelled worktree create","outcome":"refused",
  "message":"Plugin kontor before workspace.create failed: Kontor workspace guard refused: Kontor placement registers an existing directory; worktree creation is outside it"},
 {"name":"labelled directory without projectId","outcome":"refused",
  "message":"Plugin kontor before workspace.create failed: Kontor workspace guard refused: Kontor placement requires an explicit projectId"},
 {"name":"interactive worktree create","outcome":"created"}]}
```

CLI cases, run in the disposable daemon from a real labelled seat's
environment (`PASEO_AGENT_ID`, `PASEO_WORKSPACE_ID`, `PASEO_AGENT_CWD` set to a
seat carrying `kontor.seat_binding_id`, `kontor.workspace_id` and
`kontor.worktree`). Both paths create today because neither CLI carries a
caller on the `workspace.create` request:

```json
{"name":"cli workspace create (labelled seat env)","outcome":"created","must_be":"refused after ASMA-8530"}
{"name":"cli run --new-workspace (labelled seat env)","outcome":"created","must_be":"refused after ASMA-8530"}
```

ASMA-8530 must turn both CLI cases into refusals once Paseo carries caller
identity on those paths. Until then the closure run records them as known
bypasses, not as passes.

Uninstall readback and restored behaviour:

```json
{"plugins":0}
{"expected":"unguarded","checks":[
 {"name":"interactive directory create","outcome":"created"},
 {"name":"labelled worktree create","outcome":"created"},
 {"name":"interactive worktree create","outcome":"created"}]}
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

## MCP server composition — no collision with a Kontor-composed seat

The plugin composes its own server as `kontor-observer` and skips it entirely
when the request already has a `kontor` server. A Kontor-composed scoped seat
therefore keeps its own server at its own tier and never receives a second
realm-wide observer server.

Source probe of the installed Paseo 0.11.2 bundle:

- **Codex** — `buildCodexInnerConfig` in
  `dist/server/server/agent/providers/codex-app-server-agent.js` builds
  `innerConfig.mcp_servers` from the request's `mcpServers` and passes it as a
  config override; a request entry named `kontor` would replace the provider
  home's `[mcp_servers.kontor]` as a whole. Skipping the observer entry when
  `kontor` is present is what avoids that.
- **Claude** — `claude/agent.js` passes the request `mcpServers` as explicit
  SDK options, which take precedence over a same-named settings entry.
- **opencode** — `opencode-agent.js` registers each request server through
  `client.mcp.add({ name, config })`; which entry wins for a name that already
  exists in the provider config is not recorded here. The plugin's own name
  differs, and it is not added beside a `kontor` entry.

The unit test "leaves a Kontor-composed seat untouched" pins the request-level
behaviour: a request `kontor` entry is preserved and no `kontor-observer` is
added beside it.

## Residual blocker — Paseo 0.11.x workspace creation paths

The guard sees a caller only when the request carries one. At 0.11.2 Paseo
carried the caller as `agent.callerAgentId` on `workspace.create` and passed
it to the hook (`WorkspaceInitialAgentSchema` keeps the field from
`AgentCreateRequestSchema`), and the guard reads it. The final 0.11.4 closure
run still creates on the three paths below:

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
  ASMA-8530 owns making both CLI cases refused in the closure run.
- **Next action:** widen the upstream ask: pass the caller on every
  `workspace.create` request, including MCP `create_workspace` and the CLI
  (`paseo run --new-workspace`, `paseo workspace create`), or route those
  paths through the hook.
- **Resumption trigger:** a Paseo release that carries caller identity on all
  three paths, or routes them through the hook; re-run
  `apps/paseo-plugin/scripts/closure-suite.sh` with a caller-labelled seat and
  require the two CLI cases to be refused.
- **Interim:** keep `create_workspace` out of ECP/seat tool surfaces per the
  QNR-P1-06 containment note.
