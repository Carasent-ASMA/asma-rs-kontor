# Paseo 0.8.0 fixture manifest

Every file in this directory is a **sanitized recording of the live Paseo 0.8.0
daemon**, or a variant derived from one by a named field edit. Structure,
envelopes, response types, cursors, ranges, flags and ordering are the daemon's;
every content-bearing value is synthetic.

The identity files (`protocol/server-info.json`, `protocol/daemon-status.json`,
`cli/version.txt`) and the version boundaries (`newer-app-version.json`,
`unsupported-app-version.json`, `server-info-degraded.json`) were re-captured
from 0.8.0 on 2026-09-13. The remaining fixtures were inherited from the 0.3.1
recording and re-validated against that capture: the shape audit below found
0.8.0 a strict superset of every surface the adapter reads, with no field
removed, renamed or retyped.

## Runtime identity at capture

| What | Value |
|---|---|
| Bundled CLI | `0.8.0` (`paseo --version --json`, which prints the bare string) |
| Daemon | `0.8.0` |
| Endpoint | `ws://127.0.0.1:6767/ws` |
| Health | `GET /api/health` → `{"status":"ok",…}` |
| Hello | `protocolVersion: 1`, `appVersion: "0.8.0"`, `clientType: "cli"`, capabilities `selective_agent_timeline`, `timeline_replacement_invalidation` |
| Permissions | `daemon.read`, `daemon.manage`, `tunnel.manage`, `access.manage`, `workspace.read`, `workspace.write`, `workspace.manage`, `automation.manage`, `hub.execute` |
| Features | 71 advertised; `protocol/server-info.json` records each by name |

## Capture command

One bounded, read-only WebSocket client (Node 24 native `WebSocket`), one
connection. It sent the hello and then only non-mutating requests:

```text
daemon.get_status.request
project.list.request
fetch_workspaces_request            filter.projectId=<each project in turn> page.limit=5
fetch_agents_request                filter.includeArchived=false page.limit=3
fetch_agent_request                 (the first agent id on that page)
fetch_agent_timeline_request        direction=tail limit=3 projection=canonical
agent.timeline.set_subscription.request
fetch_agent_request                 agentId="agt_kontor_does_not_exist"   (negative probe)
```

The probe created, changed, archived or stopped no project, workspace, agent,
session or process. `agent.timeline.set_subscription.request` alters only the
capturing connection's own subscribed set, which is discarded when the socket
closes.

## Shape audit: 0.8.0 against the 0.3.1 recording

Key-set diff of every captured answer against its fixture, and of the timeline
item union against the installed protocol schema:

* The pushed `status` payload gained `permissions`, so a connection's
  authorization is now stated rather than inferred.
* The feature object grew from 46 to 71 names; none was removed. The additions
  are `agentRequestReceipts`, `hubAgentRpc`, `directorySync`, `workspaceLabels`,
  `workspaceSetupRun`, `providersSnapshotCwd`, `daemonConfigReload`,
  `pushTokenRevocation`, `plugins`, `pluginManagement`, `pluginGitManagement`,
  `pluginLogs`, `pluginThemes`, `pluginSettings`, `pluginTimelineItems`,
  `skillManagement`, `workspaceTerminals`, `providerSubagentNesting`,
  `workspaceMarkUnread`, `importSessionSearch`, `explicitEventSubscriptions`,
  `fsEntryOps`, `fsEntryDuplicate`, `checkoutDiscardChanges`, `agentProfiles`
  and `agentConfigApply`.
* A directory entry gained a `project` sibling beside its `agent`; the agent
  snapshot gained `persistence.metadata`. The adapter reads the snapshot and
  ignores both.
* A timeline entry gained an optional `turnId`; the item union admits
  `permission_requested` and `permission_resolved` — classified to the same
  event kinds as the stream — alongside provider notifications, compaction rows
  and plugin items.
* No captured answer lost a field or changed a type.

## Schema authority

The DTOs were derived from the installed daemon's own schemas, not from the
capture alone. SHA-256 of the inspected sources, extracted read-only from
`/Applications/Paseo.app/Contents/Resources/app.asar`:

```text
d37ff815740daa6e5cfae039ca7c347a3e0f2c6a03b18f80311bf32fbea09a97  node_modules/@getpaseo/protocol/dist/messages.js
950d54b0d9195c4da6e9a1f23108d8984ac2c791f56dba4135329e28686ea96d  node_modules/@getpaseo/server/dist/server/server/websocket-server.js
a6431990f83670125dc2a648a11bec486c8aae4fc886f171d2e694a895c0e389  node_modules/@getpaseo/server/dist/server/server/session.js
d68f1775d5db2ef5acdc67090b16a04c5842141ef6e2f86cb36f70851a172c76  node_modules/@getpaseo/protocol/dist/client-capabilities.js
09bc06d15d8edbc2cdaf9cefbfdb9fb62691488998e3c3ec76422a063eecff01  node_modules/@getpaseo/protocol/dist/agent-lifecycle.js
```

0.8.0 moved these sources under `node_modules/` inside the archive, so the
KON-MVP-20 anchors recorded for the 0.3.1 layout no longer apply; the hashes
above are the schema authority for this baseline.

## Sanitization rules

Applied by key while walking each captured frame, so the shape is the daemon's
and the values are ours:

| Key | Replacement |
|---|---|
| `serverId`, `hostname`, `version` | `srv_kontor_fixture`, `kontor-fixture-host`, `0.8.0` |
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

* `server-info-degraded.json` — the 0.8.0 identity with features reduced to
  `providersSnapshot`, so the missing-required-feature path is judged at the
  floor.
* `unsupported-app-version.json` — `version: "0.7.9"`, just below the 0.8.0
  floor. The build stays observable and nothing is driven on it.
* `newer-app-version.json` — `version: "0.9.0"` carrying the full 0.8.0 feature
  set, so a build above the floor is still driven.
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
* `cli/*` — the 0.8.0 CLI's own JSON row shapes, read from the bundled CLI
  sources (`workspace create` → `{workspaceId, project, name, isolation, cwd}`,
  `agent run` → `{agentId, status, provider, cwd, title}`, `agent stop` →
  `{stoppedCount, agentIds}`, `agent archive`/`workspace archive` →
  `{…Id, status, archivedAt}`, `agent reload` → `{agentId, status,
  timelineSize}`). `cli/version.txt` is text, because that is what
  `--version --json` prints; on 0.8.0 it is the bare string.

## Regenerating

Validate every sanitized file against the Rust 0.8.0 DTOs and replay it through
`PaseoTransport` rather than straight into normalization:

```sh
cargo test -p kontor-runtime-paseo
```

The strongest check is the opt-in live conformance suite, which reads a real
daemon instead of a recording:

```sh
KONTOR_PASEO_LIVE=1 \
KONTOR_PASEO_HOST='127.0.0.1:6767' \
KONTOR_PASEO_ENDPOINT='ws://127.0.0.1:6767/ws' \
KONTOR_PASEO_EXECUTABLE=/Applications/Paseo.app/Contents/Resources/bin/paseo \
cargo test -p kontor-runtime-paseo --test live -- --ignored --nocapture
```
