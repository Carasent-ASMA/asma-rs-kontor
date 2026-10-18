# Kontor Paseo plugin

The Kontor-owned Paseo plugin. It provides the native Sessions navigator and
the KR-11 foundation of the Kontor relaunch plan (ASMA-8507): the decided
binding hooks plus the `before("workspace.create")` placement guard. The
navigator reads Paseo directly and works without `kontord`.

## Sessions navigator

The plugin adds one **Sessions** screen, one sidebar entry and one global
Command Center entry. All three are navigation affordances only:

- **Complete inventory.** The screen walks every page of the selected host's
  native agent directory (200 rows per page) before it reports the inventory
  as current. The default scope is every non-archived session, including
  closed, idle, error, running and initializing sessions, unlabelled or ad-hoc
  work and both Kontor orchestration surfaces. Workspace or project membership
  is never an enrollment filter. An advanced option includes archived sessions;
  archived (`archivedAt`) stays visually separate from the native `closed`
  state.
- **Live native truth.** Initializing, running, idle, error and closed come
  from the host. Attention is a separate signal with its reason (`finished`,
  `error`, `permission`) when the host supplies it. A subscription streams
  native `agent_update` upserts and removals.
- **Reconnect reconciliation.** A dropped host marks the visible rows stale and
  stops claiming to be live. A new subscription snapshot triggers a fresh page
  walk before the directory is current again. Failed refreshes keep the last
  rows visible with the failure and a Retry action.
- **Search and filters.** Search matches every token against the full original
  session title, workspace name, project name/key and cwd. Native state and
  attention filter independently; the advanced area shows an active-filter
  count and a clear action.
- **Open the existing session.** A row opens by native agent ID through the
  host's navigation API. Duplicate titles never select by name. Missing
  sessions, older hosts without plugin navigation and host failures produce a
  recoverable message instead of opening a different conversation.
- **Return entry.** Opening a session pushes the host's own session route, so
  the native close/back returns to the plugin screen. The sidebar entry and the
  global **Open Kontor Sessions** Command Center item remain available from the
  session room. The screen restores its search, filters and scroll offset for
  that client session.

The screen is a React Native plugin screen. It does not replace Paseo's native
sidebar, footer, tab strip, session room or composer, and it does not embed the
workshop HTML or depend on DOM, SVG, WebView or canvas imports.

## Client authority boundary

The navigator borrows the selected host's connection through
`getPaseoClient(hostId)`. It receives no Kontor credential, reads no realm
state, and calls no `kontord` API. Client code only reads the native agent
directory, manages its own display state and calls the host's
`navigation.openAgent`. It has no prompt, interrupt, permission, schedule,
workspace-write or workflow-write call. The server hooks are unchanged.

Search, filters and scroll offset are in-memory view state for the current
client session. They are navigation preferences, not workflow state, and they
grant no authority. Saved organization, drag order and cross-client preference
merging remain KN-02 work.

## Capability findings (2026-10-18)

Observed on this workstation:

- Paseo app and daemon/CLI `0.11.4`; the plugin SDK client contracts are
  identical between the `0.11.2` floor and `0.11.4`, and the plugin now pins
  `0.11.4`. The manifest requires `paseo >= 0.11.4`.
- The native directory API supports a complete page walk, a subscribed
  `fetch_agents_result`, native `agent_update` upserts/removals and a second
  subscription snapshot after a daemon restart. A disposable-daemon probe
  created a session, observed its title update through the subscription and
  read every page with a `limit: 1` walk.
- `navigation.openAgent` is the supported native open. The `0.11.4` host maps
  it to its own `navigateToAgent`, which resolves the workspace from the agent
  ID and pushes the native session route. The plugin screen route is pushed as
  well, so native close/back returns to it.
- **Reorder gap:** the plugin SDK exposes no reorder, drag or move primitive,
  and no plugin-safe gesture package. The only supported pointer/touch route is
  React Native's `PanResponder` and responder props from the allowed
  `react-native` module. That is enough to implement touch reorder, but it is
  not a host-managed reorder contract; KN-02 must provide both the gesture and
  the accessible move controls, and KN-03 must qualify them on a physical
  iPhone. No reorder workaround was added to KN-01 because it would change the
  KN-02 acceptance surface.
- The subscription API and complete page walk were exercised against an
  isolated daemon. The host's own plugin compiler bundled `index.client.tsx`
  and `index.server.ts`; the compiled client bundle executed under the host
  module allowlist and registered the screen, sidebar item and Command Center
  item. The desktop host bundle's navigation mapping (`openAgent` to its native
  `navigateToAgent`) and the plugin-screen route push were inspected; open-by-ID
  behavior is pinned by unit tests, not by a device run. A physical iPhone was
  not available in this checkout; device acceptance remains a KN-03 gate rather
  than a claim here.

## Hooks

| Hook | Behaviour |
| --- | --- |
| `before("agent.create")` | Adds one stable system-prompt line and the `kontor-observer` MCP server at **observer** authority. A server already named `kontor` is preserved, so Kontor-composed seats are untouched. |
| `before("agent.session_open")` | Reads the session's own labels from the local daemon and sets `KONTOR_SEAT` from the keys Kontor actually writes (`kontor.seat_binding_id`, `kontor.role_slot_id`, or `kontor.agent-run`), or `KONTOR_SEAT=unbound`. A failed or slow read sets `KONTOR_SEAT=unresolved`. The key is rewritten on every open, so an old seat never survives a resume. It never blocks the open. |
| `before("workspace.create")` | Refuses a labelled Kontor seat's explicit workspace creation outside that seat's own placement: its bound project and canonical worktree. Both the initial agent's labels and the caller carried on the request (`agent.callerAgentId`) are checked. Interactive and unlabelled creation is unchanged, and an unreadable caller lookup with no seat labels on the request fails open. |

The plugin adds no UI, no menu and no agent-creation path.

## Kontor placement

A labelled seat may register an existing directory only inside its own binding:
the native project its own workspace belongs to and the canonical worktree
named by its `kontor.worktree` label. The binding is read from the seat's
`kontor.workspace_id` and `kontor.worktree` labels plus a read-only Paseo
workspace read. Everything else is refused as outside Kontor placement:

- a Paseo-owned worktree source (`kind: "worktree"`);
- a directory without an explicit `projectId` (Paseo would infer a project);
- a relative directory path;
- a directory inside Paseo's own `~/.paseo/worktrees` root;
- a project other than the seat binding's native project;
- a directory other than the seat binding's canonical worktree;
- a directory outside `KONTOR_PLUGIN_PLACEMENT_ROOTS`, when configured;
- an unresolved binding for a labelled seat.

The plugin resolves the binding read-only and holds no Kontor write credential;
`kontord` still owns the native binding and readback.

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

It starts a disposable daemon home on a free loopback port with
`KONTOR_PLUGIN_MCP=off`, installs the plugin from this directory, proves the
native session subscription and complete page walk against that daemon, proves
the guard and the interactive path, removes the plugin, and proves the refused
create succeeds again. No `kontord` process or Kontor credential is present.

### Running without kontord

The navigator does not need the observer MCP. Start the daemon with the
existing switch:

```bash
KONTOR_PLUGIN_MCP=off paseo daemon start --home ~/.paseo
```

The hooks and placement guard stay active; only the composed MCP server is
skipped. Open **Sessions** from the plugin sidebar. To check the installed
revision and the host's scope, use the header line in the screen (host label,
archive scope, session count and pages read).

### Using the screen

1. Open **Sessions** in the plugin sidebar. The header shows the selected host,
   archive scope, session count and whether the directory is live, stale or
   unavailable.
2. Type in the search field to match the full session title, workspace,
   project or cwd. Use the state chips and **Filters** for native state,
   attention and archived sessions.
3. Select a row to open that exact native session. Return with the native
   close/back action, the sidebar entry or the global **Open Kontor Sessions**
   Command Center item.
4. If the host disconnects, the rows stay visible with a stale banner. Retry or
   wait for reconnect; the next subscription snapshot reconciles every page
   before the screen reports **Live** again.

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

`test/session-directory.test.ts` covers a 505-session three-page walk, repeated
and missing cursors, native upserts/removals, attention, archival scope,
disconnect staleness, reconnect reconciliation and source failures.
`test/session-actions.test.ts` covers duplicate-name open by native ID and the
missing/navigation/failure cases. `test/client-entry.test.ts` pins the screen,
sidebar and Command Center contributions. `test/security.test.ts` extends the
floor checks to the client files: no Kontor credential, no `kontord`, no
file/process access and no agent-control or workflow-write call.

The isolated closure suite's `navigator-probe.mjs` creates one disposable
session, subscribes to the native directory, verifies a `limit: 1` complete
page walk and observes a native title update through the subscription.

## Known limit — Paseo 0.11.x

The guard covers an explicit `workspace.create` request whose initial agent
carries Kontor labels, and the caller carried as `agent.callerAgentId` on
that request. Paths that carry neither a labelled
agent nor a caller still bypass the guard:

- the Paseo MCP `create_workspace` tool does not run the `workspace.create`
  hook at all; `create_workspace` in the tool host calls the provisioning
  service directly (`packages/server/src/server/agent/tools/paseo-tools.ts`
  and `bootstrap.ts`), so the plugin never runs for that path.
- `paseo run --new-workspace` creates the workspace before it creates the
  agent, with no agent and no caller on the `workspace.create` request.
- `paseo workspace create` creates a workspace with no agent and no caller.

The same three paths remain in Paseo 0.11.4. Until Paseo supplies a caller on
every workspace creation and routes the MCP tool through the hook, a labelled
seat can still bypass the guard with the MCP `create_workspace` tool or either
CLI path. That residual is recorded in the
ASMA-8507 evidence with the widened upstream action; G-20 is not closed by the
hook alone.
