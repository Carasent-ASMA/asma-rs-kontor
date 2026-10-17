# ASMA-8507 — Kontor Paseo plugin floor and workspace guard

> **Date:** 2026-10-17
> **Base:** `0971674d` (`origin/master`)
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
| `apps/paseo-plugin/server/placement.ts` | Kontor placement shape check. |
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
     Tests  38 passed (38)
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

## No write credential

- The composed MCP server is always `--credential-tier observer`; the tier is
  not configurable (`server/config.ts`, `DEFAULT_CREDENTIAL_TIER`).
- Production plugin code contains no `credentials.json`, `KONTOR_AUTH`,
  `admin`/`operator` tier, `daemon-keypair`, HTTP write or child-process call.
  `test/security.test.ts` fails if one is introduced.
- The plugin names the realm state root but never opens it.

## Residual blocker — Paseo 0.11.2 hook input

The hook cannot identify a *caller* seat in Paseo 0.11.2, and the Paseo MCP
`create_workspace` tool bypasses the hook:

- `@getpaseo/protocol` 0.11.2 `WorkspaceCreateRequestSchema` has no top-level
  `callerAgentId`; the lifecycle request schema is `.strict()`, so a caller
  field cannot reach the hook even if a client sends it.
- The Paseo tool host's `create_workspace` calls
  `workspaceProvisioning.createWorkspaceForDirectory` directly
  (`packages/server/src/server/agent/tools/paseo-tools.ts` and
  `bootstrap.ts`), without `pluginRuntime`.

The guard therefore enforces on the labels the hook does see: the initial
agent's `agent.labels` on an explicit `workspace.create` request, plus a
caller id if a future release supplies one. A labelled seat can still bypass
the hook with the MCP `create_workspace` tool. G-20 is not fully closed by
this hook until Paseo routes the MCP tool through the hook or supplies caller
identity.

- **Owner:** Kontor LSA for the upstream ask; Paseo upstream for the change.
- **Next action:** file the upstream ask to pass `callerAgentId` in the
  `workspace.create` before-hook request (or route the MCP tool through the
  same hook).
- **Resumption trigger:** a Paseo release that carries caller identity in the
  hook or routes the MCP tool through it; re-run
  `apps/paseo-plugin/scripts/closure-suite.sh` with a caller-labelled seat.
- **Interim:** keep `create_workspace` out of ECP/seat tool surfaces per the
  QNR-P1-06 containment note.
