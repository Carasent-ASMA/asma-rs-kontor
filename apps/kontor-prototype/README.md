# Kontor plugin review prototype

An isolated review build of the Kontor Paseo plugin. It renders the plugin's own
client source (`apps/paseo-plugin/index.client.tsx` and everything it imports) on
react-native-web, behind a fixture host. Every session, plan and Jira capture is
fictional. Nothing here connects to Paseo, and nothing here is installed into
Paseo.

This is research and prototype source. Production code lives only in
`apps/paseo-plugin`. The prototype imports that code and never changes it.

## Open it

From the repository root, `pnpm install` installs the prototype together with the
rest of the workspace. Then:

| Use | Command (in `apps/kontor-prototype`) | URL |
| --- | --- | --- |
| Live editing | `./node_modules/.bin/vite --host 127.0.0.1` | http://127.0.0.1:58091/ |
| Review build | `./node_modules/.bin/vite build && ./node_modules/.bin/vite preview --host 127.0.0.1` | http://127.0.0.1:58092/ |
| Typecheck | `./node_modules/.bin/tsc --noEmit` | |

Both servers bind to 127.0.0.1, so they are reachable only from this machine.

The studio page shows the scenario list, a phone frame (390×844, touch pointer)
and a desktop frame (1440×900, mouse pointer).

Query parameters:
- `scenario=<id>`
- `theme=dark|light`
- `layout=both|phone|desktop`

A single frame opens with `?embed=1&scenario=<id>&pointer=coarse|fine`. **Reset frames**
reloads both frames from their fixtures.

## Scenarios

| id | What it shows |
| --- | --- |
| `baseline` | The default tree with one epic open. |
| `filtered-errors` | The Error filter shows flat results, each with its placement. One idle session carries an earlier error cue. |
| `needs-input` | The Needs input filter: a task QA session and a desk reviewer. |
| `provenance` | A search for "swe" that matches sessions across epics, a desk, a session without a workspace and a session with no labels. |
| `empty-results` | A search that matches nothing. |
| `clear-restore` | 84 sessions. Open some groups, scroll, filter, then clear the filter: the open branches and the scroll position come back. |
| `board-tasks` | Board for plan ASMA-99101. Tasks have zero, one or several linked sessions, and a replacement SWE session counts as earlier. |
| `parallel-roles` | Board for plan ASMA-99102: two SWE sessions and two QA sessions work on one task at the same time. |
| `simulated-updates` | A script changes native states every 4 s, and the banner says the changes are simulated. |
| `host-offline` | The host drops after 4 s. Board then shows the last known states. |
| `no-sessions` | An empty host, so no Board task has a linked session. |

## Shared and substituted code

These come from production, unchanged:
- the plugin's `contribute()` entry and every screen, row, menu and test ID;
- the plugin's RPC contracts, called through the SDK's own `callPluginRpc`;
- `@getpaseo/plugin` types, the client and host helpers, React, react-query and zod.

These are substitutes in `src/host`:
- `sdk-client.ts`: a stand-in for `@getpaseo/plugin/client`. It provides a fake
  Paseo client (fixture agents, workspaces and live updates) and `useHosts`.
- `sdk-react-native.tsx`: a stand-in for `@getpaseo/plugin/client/react-native`. It
  provides FlatList, ScrollView, TextInput and Modal from react-native-web, and
  draws Icon with lucide-react, resolving lucide's exported aliases such as
  `Building2`. It also logs
  toasts and copied text to the host log.
- `sdk-ui.tsx`: a `SidebarRow` drawn roughly like Paseo's own.
- `react-native.ts`: react-native-web, with `Linking` sending links to the host log.
- `runtime.ts`: the fixture RPC server for settings, sources and Jira captures. It
  also runs the scripted simulated updates.
- `theme.ts`: Paseo dark and light colours, copied from the host theme. The
  colours and fonts are an approximation.

Every host side effect lands in the frame's **Host log**: settings saves, opening a
session or workspace, links, the clipboard and toasts. Each one is also printed as
a `[kontor-prototype] <kind> <detail>` console line, so browser checks can count
them.

## Native preview path (not run)

The same client source compiles with Paseo's plugin compiler (esbuild, CommonJS,
with the host's modules left external) and runs in the Paseo iOS and Android apps.

A native preview would install a compiled build into an isolated test daemon
(never the 58055 install or production `~/.paseo`), with fixtures supplied over
the plugin's RPC. That daemon would then be opened from the Paseo app on a device.

This iteration did not do that. No native or device check has been run.

These remain to be checked on a device:
- VoiceOver and TalkBack labels and focus order;
- hardware Back, and Android Back closing menus;
- safe areas;
- real touch targets;
- the `scrollToIndex` placement when Show in tree is used.

## Parity map: historical HTML prototype

Reference snapshot:
`_docs/ai-orchestration/assets/2026-09-19-00-10-kontor-workshop-prototype.html`
(SHA-256 `8c8dfb0d682e166cb461ba16b03917edbce3f1b0d569fb202b6acd9c2346f995`). It
is kept as a reference and is not maintained alongside this prototype.

| HTML view | In this prototype |
| --- | --- |
| Sessions (desks, epic and project tree, filters, favourites) | The delivered Sessions screen, plus flat filtered results, Show in tree and Clear restoring the tree. |
| Plan, Graph, Delivery | The delivered Plan, Graph and Delivery pages with fixture sources. |
| Board (not in the HTML) | The delivered compact Board, plus linked sessions for each task and a role line. |
| Teams and skills | The delivered "AI teams & sessions" page under More. The HTML's team editor (seat transitions, skills, rungs) is not in the plugin. |
| Settings | The delivered Settings page. Saves go to the host log only. |
| Home, ledger, scope, decisions, Adam desk, kickoff and rehearsals | Not in the delivered plugin, so not reproduced. Overview, Decisions, Activity and Inbox show the plugin's own pages. |

## Known limits

- Team, workflow and risk tier: no configured source declares them for a task, so
  the prototype shows them as not declared. Roles come only from the
  `kontor.role` or `role` session label. Their display order implies no handoff.
- A task links to a session only when one of these session labels exactly equals
  the task's key: `jira.issue`, `task` (both against the Jira key) or
  `kontor.ticket` (against the Kontor ticket). Session titles are never read.
- At 390×320, the compact toolbar, a filter chip and the pinned Show in tree bar
  leave almost no room for the list. The rows are reached again after Back.
- Any anchored menu logs "BackHandler is not supported on web" on
  react-native-web. The delivered More menu logs the same message.
