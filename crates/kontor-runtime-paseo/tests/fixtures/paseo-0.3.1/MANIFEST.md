# Paseo 0.3.1 fixture manifest

Every file in this directory is a **sanitized recording of the live Paseo 0.3.1
daemon**, or a variant derived from one by a named field edit. Structure,
envelopes, response types, cursors, ranges, flags and ordering are the daemon's;
every content-bearing value is synthetic.

## Runtime identity at capture

| What | Value |
|---|---|
| Bundled CLI | `0.3.1` (`paseo --version --json`, which prints the bare string) |
| Daemon | `0.3.1` |
| Endpoint | `ws://127.0.0.1:6767/ws` |
| Health | `GET /api/health` → `{"status":"ok",…}` |
| Hello | `protocolVersion: 1`, `appVersion: "0.3.1"`, `clientType: "cli"`, capability `selective_agent_timeline` |

## Capture command

One bounded, read-only WebSocket client (Bun native `WebSocket`, no hand-written
framing), 12-second hard deadline. It sent the hello and then only non-mutating
requests:

```text
daemon.get_status.request
project.list.request
fetch_workspaces_request            page.limit=5
fetch_agents_request                filter.includeArchived=false page.limit=3
fetch_agent_request                 (an agent id observed in the directory)
fetch_agent_timeline_request        direction=tail limit=3 projection=canonical
agent.timeline.set_subscription.request
fetch_agent_request                 agentId="agt_does_not_exist"   (negative probe)
```

The probe created, changed, archived or stopped no project, workspace, agent,
session or process. `agent.timeline.set_subscription.request` alters only the
capturing connection's own subscribed set, which is discarded when the socket
closes.

Responses arrived **out of request order** (`fetch_agents_response` before
`fetch_workspaces_response`, `daemon.get_status.response` last), which is the
multiplexing the correlation rules exist for.

## Schema authority

The DTOs were derived from the installed daemon's own schemas, not from the
capture alone. SHA-256 of the inspected sources, extracted read-only from
`/Applications/Paseo.app/Contents/Resources/app.asar`:

```text
ab53c7dd5644f409ee8b1dd017c519e81228f8d8ed0f726963b4f00e3a886c21  @getpaseo/protocol/dist/messages.js
a8b1d9b106e2d5c00d51f17078fef6516a2c54355df3a4028b8523e71f774d0c  @getpaseo/server/dist/server/server/websocket-server.js
dd892fe0c1b20239cdf8d0249028f9e72670cd93a0edda6e5ba3a22b3b1ef663  @getpaseo/server/dist/server/server/session.js
cc4b6e8e1299ad568db88a6bb1f4b7856df4f0c67e3e08b46fafdcb9a81ad843  @getpaseo/protocol/dist/client-capabilities.js
fb275fd9b9144e83c96e359f3e580c62a2a880b8b6411ebf01142af5b2252e54  @getpaseo/protocol/dist/agent-lifecycle.js
```

The first two match the integrity anchors recorded by the KON-MVP-20 replay
spike, so this rebaseline read the same authority that spike did.

## Sanitization rules

Applied by key while walking each captured frame, so the shape is the daemon's
and the values are ours:

| Key | Replacement |
|---|---|
| `serverId`, `hostname`, `version` | `srv_kontor_fixture`, `kontor-fixture-host`, `0.3.1` |
| `projectId` / `projectKey` / `project*Name` | `prj_epic` / `github.com/kontor/epic` / `Epic · ASMA-7744 · Kontor MVP` |
| `workspaceId`, `id`, `agentId`, `agentIds` | `wks_task11`, `agt_implement` |
| `sessionId`, `nativeHandle` | `prov_sess_1`, `synthetic nativeHandle` |
| `cwd`, `projectRootPath`, `workspaceDirectory`, `worktreeRoot`, `mainRepoRoot`, `path` | `/w/epic/task-11` |
| every timestamp key | `2026-08-10T09:00:00.000Z` |
| `epoch` | `8f2b1c34-0000-4000-8000-000000000001` |
| `text`, `content`, `command`, `output`, `log`, `message`, `description`, `label`, `preview`, `prompt`, `query`, file paths, diffs, URLs | `synthetic <key>` |
| `requestId` | `req-fixture` (the recorded daemon substitutes the real one at replay) |
| `endpoint`, `publicEndpoint`, `listen` | `relay.invalid:443`, `127.0.0.1:6767` |
| anything still UUID-shaped, or starting `/Users/` or `/home/` | zero UUID / `/w/epic/task-11` |

No credential, prompt, transcript, terminal output, provider handle, real path,
real project name or real agent id is retained. The committed set is scanned for
UUID-shaped and home-rooted residue after every regeneration.

## Derived variants

The negative and edge fixtures are the captured base with one named edit, so
their field sets stay live-faithful:

* `server-info-degraded.json` — features reduced to `providersSnapshot`.
* `unsupported-app-version.json` — the single negative version fixture:
  `version: "0.9.9"`. No positive 0.2.5 fixture remains anywhere in the tree.
* `workspace-*.json` — the captured directory page with `projectId`,
  `workspaceKind`, `workspaceDirectory`, `gitRuntime.isPaseoOwnedWorktree`, `id`
  or the title-borne label edited one at a time.
* `agent-*.json` — the captured snapshot with `status`, `archivedAt`,
  `workspaceId`, `cwd`, `labels`, `persistence.sessionId` or
  `pendingPermissions` edited one at a time.
* `timeline-*.json` — the captured page with `entries`, `epoch`, the
  `reset`/`staleCursor`/`gap` flags, or an entry's `sourceSeqRanges` /
  `collapsed` edited one at a time.
* `stream-*.json` — whole `agent_stream` envelopes, as the live reader buffers
  them.
* `cli/*` — the 0.3.1 CLI's own JSON row shapes, taken from the bundled CLI
  sources (`workspace create` → `{workspaceId, project, name, isolation, cwd}`,
  `agent run` → `{agentId, status, provider, cwd, title}`, `agent stop` →
  `{stoppedCount, agentIds}`, `agent archive`/`workspace archive` →
  `{…Id, status, archivedAt}`, `agent reload` → `{agentId, status,
  timelineSize}`). `cli/version.txt` is text, because that is what
  `--version --json` prints.

## Regenerating

Validate every sanitized file against the Rust 0.3.1 DTOs and replay it through
`PaseoTransport` rather than straight into normalization:

```sh
cargo test -p kontor-runtime-paseo
```
