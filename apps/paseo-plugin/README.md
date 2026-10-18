# Kontor Paseo plugin

The Kontor-owned Paseo plugin. It provides the native Kontor Sessions navigator and
the KR-11 foundation of the Kontor relaunch plan (ASMA-8507): the decided
binding hooks plus the `before("workspace.create")` placement guard. The
navigator reads Paseo directly and works without `kontord`.

## Sessions navigator

The plugin adds the **Kontor** screen, one sidebar entry, one global Command
Center command and an in-session **← Kontor** composer pill for live agents with
a workspace. The screen, sidebar and command are navigation affordances; the
pill is the in-session return control where the composer supports it.

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
  native `agent_update` upserts and removals. If the initial observation
  bootstrap fails, the last inventory stays visible and the plugin retries the
  owned observation with bounded backoff. Cleanup cancels the retry and releases
  any owned subscription; a retry never creates a second observer.
- **Reconnect reconciliation.** A dropped host marks the visible rows stale and
  stops claiming to be live. A new subscription snapshot triggers a fresh page
  walk before the directory is current again. Failed refreshes keep the last
  rows visible with the failure and a Retry action.
- **Search and filters.** Search matches every token against the full original
  session title, workspace name, project name/key, cwd and recognized
  structured labels (`epic`, `workstream`, PR and ticket identifiers). Native
  state and attention filter independently; the advanced area shows an
  active-filter count and a clear action.
- **Open the existing session.** Each row shows its native agent ID and opens
  by that ID through the host's navigation API. Duplicate titles never select
  by name. Missing sessions, older hosts without plugin navigation and host
  failures produce a recoverable message instead of opening a different
  conversation.
- **Return entry.** Opening a session uses the host's native workspace
  navigation. Paseo 0.11.4 uses `router.dismissTo` / `POP_TO` to reach the host
  workspace route, so the plugin screen is not kept underneath the session.
  Return with the **← Kontor** composer pill (for a live agent with a
  workspace), the persistent plugin sidebar entry or the global **Open Kontor**
  Command Center item. The pill opens the same host's Kontor screen; the screen
  restores its search, filters, expansion and scroll offset for that client
  session. A live agent with no workspace has no composer pill and keeps the
  sidebar and Command Center paths. The plugin does not add or control the
  native top-left Back button.
- **Archived placement limit.** The advanced archived option asks the host for
  archived agents. The host directory can omit an archived session when its
  workspace/project placement cannot be resolved. That is a host scope limit,
  not a client filter; the option still keeps `archivedAt` distinct from native
  `closed` whenever the host returns the row.

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

- Paseo app and daemon/CLI `0.11.4` observed. The plugin development/SDK pin
  is `0.11.4`; the manifest declares `paseo >= 0.11.4`. Only `0.11.4` is
  runtime-qualified (see the compatibility range below). The SDK client
  contracts used here are identical between `0.11.2` and `0.11.4`.
- The native directory API supports a complete page walk, a subscribed
  `fetch_agents_result`, native `agent_update` upserts/removals and a second
  subscription snapshot after a daemon restart. A disposable-daemon probe
  created a session, observed its title update through the subscription and
  read every page with a `limit: 1` walk.
- `navigation.openAgent` is the supported native open. The `0.11.4` host maps
  it to its own `navigateToAgent`, which resolves the workspace from the agent
  ID and uses `dismissTo`/`POP_TO` to reach the host workspace route. It does not
  push a session route on top of the plugin screen.
- The installed `0.11.4` SDK exports `addComposerPill` and `openScreen`. The
  host renders composer pills with their icon and label and keeps the label on
  compact layouts; exact ordering and overflow among built-in controls stay
  Paseo-owned. The plugin adds one **← Kontor** pill per live agent that has a
  workspace and no header button. Workspace-less agents retain the sidebar and
  Command Center paths, so composer-only is not universal one-action coverage.
  Native top-left Back remains outside plugin control.
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
  item. The host navigation mapping and its `dismissTo`/`POP_TO` route behavior
  were inspected: the plugin screen is popped rather than left under the
  session, so the composer pill, sidebar and Command Center are the return
  paths. Open-by-ID behavior is pinned by unit tests.
- **Compatibility range:** the manifest declares `paseo >= 0.11.4`. Only
  0.11.4 is runtime-qualified on this workstation. The 0.11.2 SDK client
  contracts were inspected and are identical for the used surface, but 0.11.2
  is not runtime-qualified.
- **Native client bundle:** Paseo evaluates the client bundle with
  `globalThis.eval` on Hermes. The React Native 0.81.5 Hermes compiler (HBC96)
  used by the native app rejects ES6 class syntax, so the client source is
  deliberately class-free. The actual host-compiled client bundle from a
  disposable 0.11.4 host is fetched from the plugin catalog and compiled with
  that Hermes compiler by `scripts/native-bundle-check.mjs`; the closure suite
  runs it. A disposable host compiling the pre-fix `e04fdac5` source reproduces
  `242:24: Invalid expression encountered` at `var SessionDirectory = class {`;
  the fixed bundle compiles with exit 0. Bytecode hashes embed the compilation
  source filename; record each bytecode hash with its compilation path rather
  than as a universal artifact hash.
- **KN-01 device acceptance is not performed and is not waived.** KN-01 closure
  requires opening the correct existing session on desktop and actual iPhone,
  and returning with search, filter and position preserved. KN-03 does not
  substitute for those KN-01 checks. The procedure and evidence checklist are
  recorded below; the authorized device window is separate.

### Desktop and iPhone acceptance procedure (pending)

This procedure has not been performed for this revision. It runs only in the
authorized installation/device window; browser screenshots alone do not satisfy
the desktop or iPhone check.

Record before starting:

- Paseo desktop app version and iPhone app version.
- Daemon/CLI version (`paseo --version`, `paseo status --json`).
- Plugin revision: repository commit and the installed plugin identity and
  revision (`paseo plugin ls --json`).
- Selected host `serverId`/label, `KONTOR_PLUGIN_MCP=off`, and `kontord`
  stopped or unavailable.
- Complete inventory count and pages read from the screen header.
- Two sessions that share the same title but have different native IDs and
  distinct workspace/cwd context. Each row shows its native ID, so the pair is
  distinguishable without relying on the title.

Desktop steps:

1. Open **Kontor** and record the host, archive scope, count and pages read.
2. Type a unique search term; select a state filter, the attention filter and a
   scroll position deep enough that the target row is not initially visible.
3. Record the native ID shown in the first duplicate-title row, then open that
   row. In the host session, use the tab menu **Copy agent id** and compare it
   with the recorded row ID; confirm it is not merely the same title.
4. Confirm the session's composer control row shows the **← Kontor** pill.
   Press it and confirm the Kontor screen opens on the same host with the
   search text, filter selection, expansion, scroll position and target row
   preserved. Also verify the sidebar entry and the global **Open Kontor**
   Command Center item return to the same screen.
5. Open the second duplicate-title row and confirm both its visible native ID
   and the host-copied agent ID are the distinct second ID.
6. In an authorized controlled/disposable check, remove the target session and
   confirm a recoverable “no longer available” state rather than opening another
   session by name.
7. In an authorized controlled/disposable check, disconnect and reconnect the
   host. Confirm the rows first show stale/not-live, then a complete reconciled
   inventory with no missing or duplicated rows.

iPhone steps:

1. Repeat the desktop steps on the actual iPhone app.
2. Check keyboard behavior, row touch targets, safe-area clearance, long titles
   and the compact layout.
3. Confirm the visible row ID matches the host-copied agent ID for both
   duplicate-title sessions. Confirm the **← Kontor** pill is visible in the
   native composer control row with an adequate touch target; pressing it
   preserves search, filter, expansion and position. Repeat the sidebar and
   Command Center return checks.

Evidence checklist:

- [ ] Desktop app version, iPhone app version, daemon/CLI version.
- [ ] Plugin commit and installed plugin identity/revision.
- [ ] Host ID/label, `KONTOR_PLUGIN_MCP=off`, `kontord` unavailable.
- [ ] Inventory count and pages read; no first-page-only count.
- [ ] Duplicate-title pair with both native IDs.
- [ ] Row native ID matches the host-copied agent ID on desktop.
- [ ] Row native ID matches the host-copied agent ID on iPhone.
- [ ] **← Kontor** composer pill is visible, labelled and tappable on desktop.
- [ ] **← Kontor** composer pill is visible, labelled and tappable on iPhone.
- [ ] Pill, sidebar and Command Center returns preserve search, filter,
      expansion and scroll position on desktop.
- [ ] Pill, sidebar and Command Center returns preserve search, filter,
      expansion and scroll position on iPhone.
- [ ] Missing-session and reconnect cases use controlled/disposable data only.
- [ ] Desktop pass/fail, iPhone pass/fail, tester and timestamp.

## Hooks

| Hook | Behaviour |
| --- | --- |
| `before("agent.create")` | Adds one stable system-prompt line and the `kontor-observer` MCP server at **observer** authority. A server already named `kontor` is preserved, so Kontor-composed seats are untouched. |
| `before("agent.session_open")` | Reads the session's own labels from the local daemon and sets `KONTOR_SEAT` from the keys Kontor actually writes (`kontor.seat_binding_id`, `kontor.role_slot_id`, or `kontor.agent-run`), or `KONTOR_SEAT=unbound`. A failed or slow read sets `KONTOR_SEAT=unresolved`. The key is rewritten on every open, so an old seat never survives a resume. It never blocks the open. |
| `before("workspace.create")` | Refuses a labelled Kontor seat's explicit workspace creation outside that seat's own placement: its bound project and canonical worktree. Both the initial agent's labels and the caller carried on the request (`agent.callerAgentId`) are checked. Interactive and unlabelled creation is unchanged, and an unreadable caller lookup with no seat labels on the request fails open. |

The hooks add no menu and no agent-creation path. The navigator adds the
Kontor screen, sidebar row, Command Center command and per-agent **← Kontor**
composer pill described above.

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
skipped. Open **Kontor** from the plugin sidebar. To check the installed
revision and the host's scope, use the header line in the screen (host label,
archive scope, session count and pages read).

### Using the screen

1. Open **Kontor** in the plugin sidebar. The header shows the selected host,
   archive scope, session count and whether the directory is live, stale or
   unavailable.
2. Type in the search field to match the full session title, workspace,
   project, cwd or recognized epic/workstream/PR/ticket labels. Use the state
   chips and **Filters** for native state, attention and archived sessions.
3. Select a row to open that exact native session. In 0.11.4 the host reaches
   the workspace route with `dismissTo`/`POP_TO`, so return with the **← Kontor**
   composer pill, the persistent sidebar entry or the global **Open Kontor**
   Command Center item; native back does not keep the plugin screen underneath
   the session. Search, filters, expansion and scroll offset are restored on
   return.
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
recognized epic/workstream label search, disconnect staleness, reconnect
reconciliation, obsolete-generation update fencing and source failures.
`test/session-actions.test.ts` covers duplicate-name open by native ID and the
missing/navigation/failure cases. `test/client-entry.test.ts` pins the Kontor
screen, sidebar and **Open Kontor** command. `test/return-control.test.ts`
covers per-agent pill registration, snapshot reconciliation, close/archive/
remove/workspace lifecycle, failed-bootstrap and owned-subscription-error
observation recovery, retry cleanup, ad-hoc sessions without a workspace,
late-walk fencing and opening the screen. `test/session-view-store.test.ts`
covers host-scoped query, filter, expansion and scroll restoration.
`test/security.test.ts` extends the floor checks to the client files: no Kontor
credential, no `kontord`, no file/process access and no agent-control or
workflow-write call.

The isolated closure suite's `navigator-probe.mjs` creates one disposable
session, subscribes to the native directory, verifies a `limit: 1` complete
page walk and observes a native title update through the subscription.

The closure suite also runs `scripts/native-bundle-check.mjs`. It fetches the
actual host-compiled Kontor client bundle from the disposable daemon's plugin
catalog and compiles it with the React Native 0.81.5 Hermes compiler
(`sdks/hermesc`, HBC96). This is the native parser check, not a Node or browser
parse; the bundle must reach exit 0.

## Known limit — Paseo 0.11.x

The composer pill contribution requires a workspace ID. A live ad-hoc agent
with no workspace at all cannot receive a pill; the sidebar and **Open Kontor**
Command Center item remain available. Composer pill order and overflow are
Paseo-owned and are not promised. The plugin does not control the standard
top-left Back button.

The archived option follows the host directory. An archived session can be
absent when its workspace or project placement no longer resolves; the client
does not invent a row or a workspace. When the host returns an archived row,
`archivedAt` is shown separately from the native `closed` status. The default
non-archived inventory is not affected.

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
