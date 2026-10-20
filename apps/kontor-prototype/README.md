# Kontor plugin review prototype

An isolated review build of the Kontor Paseo plugin. It renders the plugin's own
client source (`apps/paseo-plugin/index.client.tsx` and everything it imports) on
react-native-web, behind a fixture host. Every session, plan, Jira capture and
simulated team declaration is fictional. Nothing here connects to Paseo, and nothing here is installed into
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

The studio page shows one toolbar above a phone frame (390×844, touch pointer)
and a desktop frame (1440×900, mouse pointer). **Scenario** names the current
scenario and opens a searchable list, with the highlighted scenario's
description under it. **Prototype · fixtures ⓘ** gives the current scenario's
description, where the data comes from and a link to the bare embed. Escape, a
press outside, or ✕ closes either panel, and focus returns to the button that
opened it.

Query parameters (each change is a browser history entry, so Back and Forward
restore it):
- `scenario=<id>`
- `theme=dark|light`
- `layout=both|phone|desktop`: the default is `both`

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
| `team-declared` | **Simulated.** Board for plan ASMA-99101 with fixture team, workflow and risk-tier declarations. ASMA-99103: SA, then SWE, then QA and AUD in parallel. ASMA-99104: a replaced SWE, a reviewer outside the team, no SA or AUD session. ASMA-99108: a risk tier only. ASMA-99105 and ASMA-99106: not declared. |
| `team-unordered` | **Simulated.** Board for plan ASMA-99102. ASMA-99107: roles without a declared order. ASMA-99113: a declared team with no session yet. |
| `parallel-roles` | Board for plan ASMA-99102: two SWE sessions and two QA sessions work on one task at the same time. |
| `simulated-updates` | A script changes native states every 4 s, and the banner says the changes are simulated. |
| `role-view` | Role grouping is saved, and only QA is saved open. There is one header per role, including singletons (LSA, TPM, SA, JUDGE, ADAM) and No role last. QA gathers `qa`, `QA`, `Qa` and a padded `  qa ` across workstreams and workspaces, while QA1 has its own header. "AUD • …" labelled `qa` sits under QA with a conflict cue. A session labelled `kontor.role` SWE and `role` tpm sits under SWE. `rev_b` and the title "REV B • …" meet under REV B. Each session row's Details button shows its full role and place; an AUD session with a long title and workstream shows how they wrap. |
| `role-view-offline` | The same sessions, with Role picked in this view. The host drops after 4 s. Saved preferences then become unavailable, as for every view, so a grouping that was only saved would fall back to the default tree. The role headers give last known counts. |
| `host-offline` | The host drops after 4 s. Board then shows the last known states. |
| `keyboard-back` | The Back shortcut is saved as Cmd+` (Cmd+< on Norwegian), in its own keyboard document. Board for plan ASMA-99101 opens first. Open another view and press the shortcut to go back one view; it does nothing while typing in Search or with a menu or dialog open. Settings › Keyboard switches it off or on and has **Test shortcut**, which says what key reached Kontor. In a browser the page sees the key unless macOS takes it first. Every other scenario starts with the shortcut off. |
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
  keeps the organization and keyboard settings as two documents, as the host does,
  and runs the scripted simulated updates.
- `theme.ts`: Paseo dark and light colours, copied from the host theme. The
  colours and fonts are an approximation.
- `task-team-source.ts`: a stand-in for the plugin's `client/task-team-source.ts`.
  It returns the production reader's answer, except in scenarios marked
  simulated, where the keys in `fixtures/teams.ts` answer with a simulated
  declaration. See "Team, workflow and risk tier" below.

Navigation works in the prototype. Opening a session or a workspace shows the
host shim's view of it (title, state, labels, Open workspace) with a Back button,
where Paseo would open the native conversation or workspace. Host side effects
are mocked: settings saves, links, the clipboard and toasts go nowhere. The
frame's **Host log** records every navigation and every mocked effect, and each
is also printed as a `[kontor-prototype] <kind> <detail>` console line, so
browser checks can count them.

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
| Board (`vBoard`): stats, a project switch, filter chips and task rows by lane, wave or grid. A row opens the task view (`vTask`), whose Runs tab names the team and run, with a seat pipeline: one chip per role seat, each opening that seat's session. The teaching-plan fixtures give each task a High-stakes or Standard tier, shown in plan exports | The delivered compact Board, kept as is. Each task row adds one line for its sessions: role and state. In the simulated scenarios only, the line leads with the risk tier and workflow, then the roles in declared steps (" → " between steps, " & " for roles in parallel, "–" for a role with no session); production has no declaration provider. One session opens directly; otherwise a chooser lists the sessions by workspace. The HTML's task view, team snapshot and seat tiles are not reproduced. |
| Teams and skills | The delivered "AI teams & sessions" page under More. The HTML's team editor (seat transitions, skills, rungs) is not in the plugin. |
| Settings | The delivered Settings page. Saves go to the host log only. |
| Home, ledger, scope, decisions, Adam desk, kickoff and rehearsals | Not in the delivered plugin, so not reproduced. Overview, Decisions, Activity and Inbox show the plugin's own pages. |

## Team, workflow and risk tier

**What production does.** The plugin's `client/task-team-source.ts` does not
read declarations. It answers "not provided" for every task, with coverage text
built from the configured source kinds and the Teams template names. Production
has no declaration provider yet. The nine sources configured on the test host
cover these kinds, and none declares a team, workflow or risk tier for a task:

| Source kind | What it declares | Team, workflow or risk for a task |
| --- | --- | --- |
| Plan export (canonical graph v1) | Task id, title, phase, Jira key, Kontor id, dependencies, wave | No such field |
| Teams configuration: fleet bindings | Team slots bound to model chains | Not per task. Tier names appear only in YAML comments, which the parser drops |
| Teams configuration: roster | Epic seats (LSA, TPM) | Not per task |
| Teams configuration: templates | Team templates with roles and handoffs ("Plan, build and verify", "Research spike") | Not assigned to any task |
| Jira history | Key, status, status category | No |
| Decision and delivery records | Decisions; pull requests, checks and releases | No |

So production Board shows "Team, workflow and risk tier: not provided" in the
task's chooser, and Source details gives the coverage with the number of
configured sources. Session labels are never read as a declaration: an
unenforced label assigns nothing. This covers the surveyed sources only. It does
not show that `kontord` is the only possible path: Paseo-direct epics follow the
same Kontor workflow, and rendering a declaration needs no daemon.

**Vocabulary checked.** The workflow name "build-verify-deploy" does not occur in
`_docs`, `.agents`, `AGENTS.md`, `_tools/ai-orchestration` or `config` in
asma-modules, or in `docs`, `crates` or `apps/paseo-plugin` here. The nearest
names are "ASMA: plan, build, verify"
(`_tools/ai-orchestration/manifest/teams/asma-plan-build-verify.json`) and
"Plan, build and verify" (`crates/kontor-teams/fixtures/mvp-team-pack.json`).
Chore, Standard and High-stakes are policy categories, not team ids
(`.agents/skills/ticket-triage-tiers/SKILL.md`). The task workspace's slots are
SA, SWE, QA and AUD, with no handoffs declared
(`crates/kontor-profiles/fixtures/operational-domain.json`).

**What the simulated scenarios add.** `team-declared` and `team-unordered`
answer with `fixtures/teams.ts` for some fixture keys, so the design can be
reviewed with real-looking declarations. The banner says "SIMULATED team
declarations", and Source details names "Simulated team declarations (prototype
fixture)". This is a design fixture, not a source format, a backend schema or a
Kontor binding. The same plugin code renders the production "not provided" state
for every other task and scenario.

**How a declaration is shown.** The plugin ships this renderer, but only its
unit tests (with an injected reader) and the simulated scenarios reach it.
- The task line gives the risk tier and workflow, then the roles. A missing part
  is named ("workflow not declared"); a role without a session shows "–".
- Roles are placed in steps by the declared handoffs. Roles without a path
  between them share a step and are joined by " & " (parallel QA and AUD). A
  handoff back to an earlier role, such as QA returning work to SWE, does not
  reorder them. With no handoffs declared, nothing is ordered.
- A session sits under a role only by its own `kontor.role` or `role` label.
  Linked sessions whose role the declaration does not name are listed after the
  team; replaced and archived ones count as "N earlier".
- A task with a declared team always opens the chooser, which leads with the
  team, even with one or no session.
- How sessions and teams were read (label keys, the exact join, the declaring
  source) is in Source details, on Board and in the chooser, not in the main
  text.

## Known limits

- A task links to a session only when one of these session labels exactly equals
  the task's key: `jira.issue`, `task` (both against the Jira key) or
  `kontor.ticket` (against the Kontor ticket). Session titles are never read.
- Below 420pt of screen height on a phone, Show in tree keeps only its Back row
  above the list. The toolbar and filters come back with Back.
- Any anchored menu logs "BackHandler is not supported on web" on
  react-native-web. The delivered More menu logs the same message.
- This is a web build. The ASMA-8307 proposal it follows
  (`_docs/ux/architecture/2026-09-29-19-34-architecture-executable-ux-prototyping.md`)
  is In Review, not accepted. Nothing here was checked on iOS or Android; see
  "Native preview path".
