# Kontor plugin web prototype

This is a web rendering of the real Kontor plugin source on React Native Web,
with a synthetic Paseo host. It is **not the native Paseo app**. Nothing is
installed into Paseo. Reloading resets fixture sessions, settings and receipts.
Working as uses this isolated web origin's browser storage.

The entry imports `../paseo-plugin/index.client.tsx` directly. There is no copied
plugin client. [Source-sync audit](SOURCE-SYNC.md) pins the source and records
the reference delta, retained files and remaining boundaries.

## Run

From the repository root:

```sh
pnpm install --frozen-lockfile
pnpm --filter @carasent/kontor-paseo-plugin-prototype typecheck
pnpm --filter @carasent/kontor-paseo-plugin-prototype test
pnpm --filter @carasent/kontor-paseo-plugin-prototype build
pnpm --filter @carasent/kontor-paseo-plugin-prototype preview
```

The review URL is <http://127.0.0.1:58142/>; the optional editing server (`dev`)
uses port 58141. Both bind only to loopback and refuse an occupied port.
The Content Security Policy blocks all fetch/WebSocket connections. Development
HMR is disabled for that reason; reload after editing.

The studio frames the same source at phone and desktop sizes. Its toolbar selects
scenarios and light/dark appearance. Query parameters:

- `scenario=<id>`
- `theme=light|dark`
- `layout=both|phone|desktop`
- `embed=1&pointer=coarse|fine` for a single standalone frame

## Host substitutions

- `sdk-client.ts`: fixture host discovery and settings; reuses real SDK hooks/RPC
  validation, supplied with an in-memory Paseo API.
- `sdk-react-native.tsx`, `sdk-ui.tsx`, `theme.ts`: approximate host chrome,
  icons, modals, toasts and palette. Clipboard and links only enter the host log.
- `react-native.ts`: React Native Web, a web-only hardware Back no-op, the unused
  native Settings import, and logged Linking calls.
- `runtime.ts`: paginated fixture agents, projects and workspaces; source,
  registry, responsibility and Kontor-status RPCs. Unknown calls fail visibly.
- `session-lifecycle.ts`: disposable fixture reviews, archive/Resume outcomes,
  exact-ID availability, dispositions and reconciliation. These simulate effects
  only in the page's map; they do not qualify the production server behavior.
- `session-create.ts`: synthetic New LSA options and idempotent outcomes. Creation
  adds a fictional entry to the page's map; it never dispatches a native request.
- `task-team-source.ts`: delegates to the production reader except for explicitly
  marked simulated team scenarios. Those declarations are not an authoritative
  production assignment source.
- `shell.tsx`, `plugin-host.ts`: call the real contribution entry and render its
  screen. Opening a session/workspace shows a host shim instead of a conversation.

## Browser verification

Use an installed Playwright-compatible module and Chromium executable. The
checker launches its own browser with a fresh profile, accepts only the isolated
loopback preview and blocks every request except GETs to that exact origin.
It checks every scenario, current profile/sort controls, synthetic lifecycle
outcomes, overflow and six screenshots. Evidence stays outside the repository.

```sh
node apps/kontor-prototype/scripts/browser-check.mjs \
  --playwright <absolute-installed-module> \
  --executable <absolute-chromium-executable> \
  --origin http://127.0.0.1:58142 \
  --output <outside-repository-evidence-directory>
```

No native device, VoiceOver/TalkBack, packaged Electron or real host/session
check is claimed. Fonts, touch behavior and host chrome remain web approximations.
Start/Switch comes from the current pinned plugin. Its creation options and
New LSA outcome are synthetic; a created session exists only in the fixture map. Bulk takeover remains unavailable in the
pinned production client.

## Retained scenarios

All reference scenario IDs remain available. Each uses fictional data only.

| ID | Scenario |
| --- | --- |
| `session-recovery` | Native availability and archive (simulated) |
| `compact-icons` | Compact controls: combined review |
| `compact-icons-stale` | Compact controls: stale inventory |
| `baseline` | Baseline |
| `integrated-review` | Combined navigator review |
| `integrated-roles` | Combined review: Roles |
| `integrated-offline` | Combined review: stale |
| `native-desks` | Native desk grouping |
| `status-lifecycle` | Status and retained history |
| `model-usage` | Models and token reports |
| `model-usage-roles` | Models: Roles |
| `model-usage-offline` | Models: host offline |
| `model-usage-updates` | Models: scripted updates |
| `time-view` | Times: Sessions |
| `time-roles` | Times: Roles |
| `time-stale` | Times: stale inventory |
| `filtered-errors` | Filtered: errors |
| `needs-input` | Filtered: needs input |
| `provenance` | Search: provenance |
| `empty-results` | Empty results |
| `clear-restore` | Clear restores the tree |
| `board-tasks` | Board: task sessions |
| `team-declared` | Board: declared teams (simulated) |
| `team-unordered` | Board: team without order (simulated) |
| `parallel-roles` | Board: parallel roles |
| `simulated-updates` | Scripted changes |
| `role-view` | Roles: one level |
| `role-view-offline` | Roles: host goes offline |
| `host-offline` | Host goes offline |
| `keyboard-back` | Keyboard: Back shortcut |
| `no-sessions` | No sessions |
