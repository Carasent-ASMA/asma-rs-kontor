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

- **Complete inventory, with honest partial reads.** The screen walks every
  page of the selected host's native agent directory (200 rows per page) before
  it reports the inventory as current. If live updates are unavailable while
  the walk completes, the inventory is labelled partial rather than complete.
  The default scope is every non-archived session, including
  closed, idle, error, running and initializing sessions, unlabelled or ad-hoc
  work and both Kontor orchestration surfaces. Workspace or project membership
  is never an enrollment filter. An advanced option includes archived sessions;
  archived (`archivedAt`) stays visually separate from the native `closed`
  state.
- **Live native truth.** Initializing, running, idle, error and closed come
  from the host. Attention is a separate signal with its reason (`finished`,
  `error`, `permission`) when the host supplies it. Error counts and red badges
  follow the current native `error` status. A retained error notice after the
  session starts, recovers or closes is shown as **Earlier error · now …**;
  it does not inflate Error or Attention. **More → Earlier errors** finds these
  sessions; **Include archived sessions** also brings archived records into that
  search. Closed sessions with no `archivedAt` remain in the default inventory,
  including those in archived workspaces. Other unread attention signals keep
  their native meaning. A subscription streams
  native `agent_update` upserts and removals. If the initial observation
  bootstrap fails, the last inventory stays visible and the plugin retries the
  owned observation with bounded backoff. Cleanup cancels the retry and releases
  any owned subscription; a retry never creates a second observer.
- **Needs input.** The installed native priority is followed: a pending
  permission or `attentionReason: "permission"` wins over the raw lifecycle even
  when `requiresAttention` is false. The row gets an amber indicator and a
  **Needs input** label, and the attention filter includes it. The raw
  `running`/`idle`/`error`/`closed`/`initializing` status stays separate. The
  derived state clears when the host resolves or cancels the permission and is
  replaced by the fresh page walk after a reconnect; it is never carried over as
  a stale badge. The badge is only as current as the host snapshot: a host that
  omits both public fields cannot be distinguished from a resolved permission.
  No question subtype is inferred.
- **Reconnect reconciliation.** A dropped host marks the visible rows stale and
  stops claiming to be live. A new subscription snapshot triggers a fresh page
  walk before the directory is current again. Failed refreshes keep the last
  rows visible with the failure and a Retry action.
- **Returning to Kontor.** Opening a session closes the Kontor screen, but the
  plugin runtime lives as long as the host connection. The host's directory and
  its subscription live with it, so a return shows the live rows at once
  without reading a page. Updates and reconnects that arrive meanwhile still
  apply. A return walks every page again only when the directory is new,
  offline or no longer live, or when the archive scope differs. The
  subscription's bootstrap page is one row, since the walk reads every row. A
  workspace and project read under 30 s old is reused with its own read time.
  Plugin cleanup releases the subscriptions.
- **Search and filters.** Search matches every token against the full original
  session title, workspace name, project name/key, cwd and recognized
  structured labels (`epic`, `workstream`, PR and ticket identifiers). Native
  state and attention filter independently; attention includes **Needs input**
  permission waits. The advanced area shows an active-filter count and a clear
  action. On compact layouts the Filters button has a visible down/up chevron,
  an active-count badge and a 44x44 pt target; Clear filters is a separate
  44 pt control in the work line at the top of the list. The compact header has
  no summary row: one count row beside **Grouped / Flat** says the matching
  count the Filters header says, followed only by what no visible control says
  (the context, search, Task, Archived history, Group and sort).
- **Open the existing session.** Each row shows its native agent ID and opens
  by that ID through the host's navigation API. Duplicate titles never select
  by name. Missing sessions, older hosts without plugin navigation and host
  failures produce a recoverable message instead of opening a different
  conversation.
- **Return entry.** Opening a session uses the host's native workspace
  navigation. Paseo 0.11.4 uses `router.dismissTo` / `POP_TO` to reach the host
  workspace route, so the plugin screen is not kept underneath the session.
  Return with the **← Kontor** composer pill (for a live agent with a
  workspace), the persistent plugin sidebar entry, the global **Open Kontor**
  Command Center item or, from an opened session's Command Center, **Back to
  Kontor**. The pill and both commands open the same host's Kontor screen.
  **Back to Kontor** does the same as **Open Kontor**; it is an agent-context
  command with the keywords back and return. The plugin SDK gives a command no
  key binding of its own. The
  existing host-scoped view store is unchanged; physical restoration on this
  candidate is not claimed and remains a pending device check. A live agent with
  no workspace has no composer pill and keeps the sidebar and Command Center
  paths. The plugin does not add or control the native top-left Back button.
- **Back shortcut (web and desktop).** Settings › **Keyboard** offers **Off**
  (the default) or **Cmd+`**: Cmd with the key at the Backquote position, which
  types < on a Norwegian layout. It goes back one Kontor view, as the Back
  button does, and acts only while Kontor is the screen in front and has focus
  (or nothing has). It leaves the key alone while typing in a field, composing,
  holding the key, with a Kontor menu or dialog open, when the host or a menu
  already handled the key, and when there is no view to go back to. macOS can
  use the same keys for "Move focus to next window", so it may take them first.
  **Test shortcut** waits ten seconds for one key pressed with Cmd in Kontor and
  says what reached Kontor: the key and its reported position, Cmd alone, or
  nothing. It does not name a cause it cannot see. It leaves typing, repeats,
  input methods and keys without Cmd alone, stops when focus, a click or the
  window leaves Kontor, claims only the Back shortcut's own key so Back does not
  act during the test, and writes no setting. **Cancel test** stops it, and
  **Copy result** copies the result. The choice is a separate host settings
  document (`keyboard`, schema version 1), shared by every client of the host;
  the organization document is untouched. Native clients show that the shortcut
  is web and desktop only, since the plugin SDK has no keyboard contract and the
  web listener reads DOM key events.
- **Compact toolbar.** Compact layouts keep one slim host/live-or-stale row with
  **Search** and **Filters** (including the active count). The full host
  summary, search field, status chips and advanced options sit at the list top.
  Deliberate downward scrolling collapses the full controls; a short reverse
  scroll, or the explicit Search/Filters action, reveals them at the current
  list position. The collapsed controls are removed from layout rather than
  leaving a spacer. Focused search/filter controls and screen-reader mode pin
  the controls open; reduce motion skips the short reveal fade. Wide layouts
  keep the existing full header without an extra persistent row; stale/error
  truth remains visible in the slim row when collapsed.
- **Archived placement limit.** The advanced archived option asks the host for
  archived agents. The host directory can omit an archived session when its
  workspace/project placement cannot be resolved. That is a host scope limit,
  not a client filter; the option still keeps `archivedAt` distinct from native
  `closed` whenever the host returns the row.

The screen is a React Native plugin screen. It does not replace Paseo's native
sidebar, footer, tab strip, session room or composer, and it does not embed the
workshop HTML or depend on DOM, SVG, WebView or canvas imports.

### Start or switch and Recent

- **One sheet.** **Start or switch** in the Kontor context row (an icon button
  on compact layouts) and the global Command Center item **Start or switch
  Kontor session…** open one sheet. With no query it lists up to 8 Recent
  sessions; typing matches every non-archived session by title, place and
  labels, Recent first, at most 20 rows, and **Show all N in Sessions** hands
  the query to Sessions unscoped. A row opens by native ID through the same
  path as Sessions. Rows carry no archive control.
- **Recently viewed (supported scope).** A user opening a session
  through Kontor's native navigation records that exact session ID and time.
  Most recent opens come first, one row per session; A → B → A shows A, B.
  The saved sequence determines order even when wall times tie or the clock moves
  backwards. Recorded times are used only for age and retention; Clear empties
  the sequence without filtering later opens against its wall time.
  Prompts, worker activity, creation/change times, **← Kontor** and **Back to
  Kontor** never add or reorder visits. Storage is scoped by the surface's
  `host.id` and the local app/browser storage profile (iOS settings or web local
  storage, key `kontor.recent@2 ` followed by the JSON host tuple; no titles).
  This is local user storage, not a detected signed-in identity; **Working as**
  is an assignment filter and never determines ownership of visits. People
  sharing one app/browser profile share its history.
  It keeps 20 visits for 14 days, pruned on reads and writes. Archived and
  absent sessions are hidden while their unexpired visits are kept, including
  during partial/offline reads. **Clear recent on this device** clears the
  selected host only. Without a device store it lasts until Paseo restarts;
  an unreadable new record stays untouched until Clear; the sheet says which.
  Source and install evidence: [Unreleased release note](../../CHANGELOG.md#unreleased).
- **Saved history.** The old `kontor.recent@1` value mixed opens and returns
  without recording their source or host. Its bytes are preserved, including
  on Clear. **Show saved history** exposes its session IDs separately, with
  unknown original host and opening times. Old timestamps are never imported
  as opens or used to order Recently viewed. No earlier opening times can be
  recovered from this record.
- **DEFERRED FUTURE WORK — native Paseo session views (Igor, 2026-10-23).**
  Openings through the native sidebar or other Paseo screens are outside the
  supported Kontor-initiated scope. This enhancement is not completed and is
  not a release prerequisite for the delivered personal Recently viewed list.
  As of 2026-10-23, the Paseo 0.12.0 plugin contract and the
  [public plugin reference](https://paseo.sh/docs/plugins/reference#screens-and-sidebar-items)
  expose navigation commands and snapshots by supplied ID, without a global
  session-view notification. A returned `openAgent(...): void` call confirms
  navigation acceptance, not that a session was actually brought into view.
  This is a current interface limit, not a permanent API-absence claim.

  Future delivery needs a public Paseo app session-view event/API usable while
  Kontor is not mounted. It must report actual views with exact host/session
  IDs and truthful viewer/device scope, and define repeat opens, background
  updates and failed navigation without treating activity as a view. It then
  needs a reviewed compatible desktop/mobile Paseo release, a capability-gated
  Kontor consumer, and native-device proof of those semantics on desktop and
  mobile.
- **Optional future work — shared phone/desktop history.** Synchronizing
  Kontor-initiated visits across devices is a separate optional feature, not
  part of this delivery. It needs an explicit user requirement, supported
  viewer identity, and a storage/synchronization design covering host/user
  isolation, duplicates, deterministic order, offline/reconnect behavior,
  privacy and cross-device acceptance. A Paseo change is not automatically
  required to synchronize visits initiated through Kontor. Native views from
  outside Kontor still need the separate view-event capability above.
- **New LSA (bounded create).** Off unless the operator writes exactly
  `{"create":"on"}` to `$PASEO_HOME/plugin-config/kontor/create.json`
  (default `~/.paseo/plugin-config/kontor/create.json`) on the host computer.
  Any other content, an unreadable file or no file keeps it off, and the sheet
  says why. The file is read on every request and never written by the plugin.
  When on, **New LSA** creates one Adam tab in a desk's `ADAM` workspace (in a
  project named `DESK • <name>`), titled `LSA • ADHOC • <topic>`. A topic has 3
  to 48 characters after NFC normalization and never contains `•`, `·` or a
  control character. The labels are `role=LSA`, `title.auto=<title>`,
  `persona=kontor-lsa-adhoc@<hash>` and `create.request=<request ID>`; never a
  `kontor.*` or orchestration label. Provider, model, effort and mode default
  to the host's `LSA` agent profile, else the provider's own defaults; a
  dangerous mode is marked. No first prompt is sent.
- **Truthful outcome.** The request is recorded on the host, under
  `$PASEO_HOME/plugin-state/kontor/session-create-v1/`, before one idempotent
  native create. Success is claimed only from an exact readback: **Created and
  verified**, or **Created with differences** naming each one, including the
  chosen workspace's working directory. A failed or
  unanswered dispatch (120 s) is **Outcome unknown** with **Check**, which reads
  the host by the request label and never dispatches again. No matching session
  leaves the outcome unknown because a delayed creation may still complete.
  An unknown outcome stays blocked until a correlated readback confirms it;
  otherwise the operator must reconcile it on the host. An unsettled or
  unreadable record blocks the next create until it is settled. **Open
  session** appears only once the session list shows the new session and the
  app exposes plugin navigation. Nothing is created while the host is offline.
- **Persona and rename.** The LSA persona goes in the system prompt. Its
  delivery is confirmed only for Claude and Codex; other providers show
  "Persona delivery unverified". A plugin cannot rename a session, so the
  persona tells the session to rename itself with Paseo MCP `update_agent`, and
  only while its title equals `title.auto`. It renames once, to
  `LSA • <KEY> • <topic>`, when it has a confirmed epic key, and sets `epic` and
  `title.auto`. It changes the topic only at a checkpoint. Once its title
  differs from `title.auto`, which means the operator renamed it, it never
  renames again.

## Organization and read-only pages

The plugin body has Sessions, Overview, Plan, Board and More on compact clients.
Desktop also exposes Graph and Delivery directly. More contains the remaining
destinations, including Inbox, Search and Settings. The compact context picker
offers All projects, observed native projects, confirmed epic keys and configured
plans on every main view, including Sessions. Native project identity is separate
from the selected epic or plan; the selector names both when applicable. Sessions,
Overview, Plan, Board, Graph and Delivery retain that selection. Sessions filters
by native project and epic, not by exact plan links: two plans in the same epic
share its sessions, counts and filters. The selector says Epic sessions while
retaining the plan for the other views. All projects explicitly clears the selection.
A concise Native · Live badge describes the reconciled native session
inventory only; Reading, Stale, offline and errors remain distinct, and configured
plan/Jira freshness is reported on their own pages. The selector, Back and inventory
status share one local toolbar. Paseo 0.12 owns its native screen title; its supported
plugin-screen contract has no picker slot (`addHeaderButton` is workspace-only).
Plan queries and positions are scoped to the host,
project, epic, exact source identity and revision. Organization placement uses
explicit display mapping, then desk, then workstream. Back retains the previous query, position and session
filters; the native **← Kontor** control returns to the last host-local view.

Sessions has separate **Layout**, **Group by** and **Sort by** choices. **Flat**
lists the complete filtered set. **Grouped** offers **Desks & workstreams**,
**Desks & workspaces** and **Role**; switching back restores the last grouping.
The labelled **Grouped / Flat** segments stay above the scrolling list on mobile
and desktop, beside the result count when present. In short compact windows,
that row scrolls at the top of the list so session rows retain usable space.
Their checked state, keyboard focus outline and hover/focus help make the choice
visible. Use Tab to reach the
selected segment, arrows to switch and Space or Enter to select. Layout is
remembered through the existing host view and saved organization preferences;
**Group by** and **Sort by** remain in Filters & view.
On mobile, the fixed Filters & view header shows how many sessions match every
active filter, beside the heading, while its controls scroll. The number is the
final matching session set, so layout, grouping, sort and collapsed groups never
change it. It is exact only from a ready, live and complete read of the chosen
Sessions or Archived history; a partial or stale read is marked, and a read in
progress, a failed first read or an assignment that cannot be evaluated shows
**…**, **—** or **Not evaluated**, never 0.
Search, state, task, assignment and association filters apply in both layouts,
and changing layout or grouping preserves those filters and the sort. Grouped
search/state/task matches open their branches temporarily; these disclosures do
not overwrite saved expansion. Flat rows retain native placement and resolved
associations, with Details and Show in tree beside the existing session action.
Native durable desks use the host's project ID, then its actual workspace
IDs, then their sessions in both tree views. Every desk session stays in its
native workspace, including sessions with different or conflicting epic, desk or
workstream declarations. Those declarations remain searchable/filterable
associations in Details; they never become workspace-like parents within a desk.
Optional role subgroups below a workspace are a presentation view. Empty native
workspaces remain visible, and a desk with none says **No native workspaces in
this desk**. Desk counts describe actual workspaces and sessions. Same-named
projects and workspaces remain separate by native ID.

Outside native desks, the workstream/epic tree and repeated workspace-kind/role
prefix groups keep their existing behavior. Explicit epic contexts and workstream
association filters explore declared associations without changing native placement.

Filters also offers searchable **Desk / workspace scope** and **Epic scope**.
The four scope shortcuts pair icons with counts and short captions (All, Review,
Adam, Outside). Their full names and scope explanations appear on hover or
keyboard focus and in accessible names/hints; touch keeps the captions. Counts
describe the current project/epic context before search and other filters.
Each shortcut has a 44 pt target, a pressed state, a focus border and an
underlined selected caption. Group rows show the primary displayed state as
count/total, such as **Running 2/85**, at every tree depth. The denominator counts
descendant sessions represented in the displayed tree, including collapsed
branches. In normal Grouped view, these are only the current search/filter
matches. **Show in tree** from Flat results retains surrounding context, so its
denominator can also include sessions outside the results' search, task or
explicit state filter. Project, desk, workstream, epic and assignment scopes
still apply, as does **Current** when selected. **Back to results** restores the
matching results. Empty groups say **No sessions**. Hover/focus help and accessible hints
explain the full displayed-state breakdown and the existing priority order;
native lifecycle and attention signals can overlap those displayed states. The
Legend provides the same scope and desk explanations on touch screens and
describes whether the current tree counts matches or retained context.
Desk rows retain their full names: PR Review uses
a pull request, Adam a bot, QA a flask and QSW a bolt; unknown desks use an
armchair. These glyphs describe presentation, never durable desk configuration.
An exact desk choice uses the native project ID, distinguishing identical names;
workstream choices keep canonical association keys, and epic choices use the same
label/mapping/Jira-capture resolver as Details. Conflicting or missing epic
evidence matches **Epic unknown**, never a title-derived epic. All three scope
dimensions compose with the other filters. Active scopes remain visible above
the list, can be removed individually, and **Clear scope** removes just these
dimensions. Empty selections and unavailable keys stay explicit until cleared.

The workstream association choices list only values resolved from sessions in the
current scope: session mapping, workspace mapping, `desk` label, `workstream`
label, native DESK project fallback, then Other work, in that order. These choices
are neither a native project inventory nor a complete registry of workstreams.
Native projects/workspaces have their own context scope and native IDs. Sessions
of non-desk projects without declarations remain under Other work and their
epic, or Epic unknown; no workstream is inferred from their titles.

A native project whose sessions settle different epics shows one row per epic.
Its Epic unknown row follows the project's last recognised row when both are in
one workstream, and stays in place when they are not. Either way the rows name
each other: how many of the project's sessions have no known epic, how many of
its workspaces have no sessions, and in which workstreams. Rows match by native
project ID, never by name. Projects whose sibling rows read alike, directly or
through another such project, are named together: one folder of each project's
root, the last one where the roots differ and below any home folder, or their
position when the roots cannot tell them apart safely or a name would make a row
read like any other sibling. Compact rows
show a group's detail under its label, in up to two lines. No session moves,
merges or hides, and no epic or workstream is assigned.

The authoritative workstream registry and its separate desk categories are not
loaded by this selector. Neither are plan frontmatter declarations or verified
epic-to-workstream links. Available choices can include unregistered names;
missing choices do not mean that a registered workstream has no work. Missing
plan headers remain unassigned, and catalog metadata alone does not propagate
plan declarations into session filters. Native desk Details explicitly says
that registry membership and epic/plan links are not checked. Native project
counts, registered workstream counts and these metadata choices describe
different things; they must not be presented as one complete inventory.

Native desk keys are `project:<native-id>/workspace:id%3A<native-id>` in both tree
views, reusing the former Desks & workspaces keys. Saved expansion/order under
those keys still applies across renames. Former hierarchy keys rooted in
`workstream:<desk>` or containing an epic match no native desk row; their branches
start in the default order/disclosure once. Those entries remain in settings for
other views and history. No settings, favorites or mappings are wiped or migrated.

Role shows one header per role across the visible scope, including roles with a
single session, and **No role** last; each session appears under exactly one.
A session's role is its `kontor.role` label, else its `role` label, else its
title prefix before the first `•`. A label wins over the title. A source that
names another role stays visible on the row as a cue, such as "title says QA".
Codes match across case, Unicode whitespace and NFC, using locale-independent
upper case; this is not full Unicode case folding. Role labels read `-` and `_`
as spaces. Punctuation and suffixes otherwise stay distinct (QA1 is not QA, and
REV A is not REV B). The title fallback is for display only: Board task links and
lanes read labels, never titles. Headers follow LSA, TPM, SA, SWE, QA, AUD, UAT,
SEC, REV A, REV B, JUDGE, then other roles by name, unless Arrange saved another
order. Within a role, sessions sort by state, then workstream, epic, workspace,
creation time and ID, so an update alone never moves a row. A header shows up
to two non-zero states, each with shape, count and word, apart from its total.
Headers start closed. Saved disclosure and Expand all still apply.

A Role row shows a short place. The **Details** button beside it shows the
whole of it:
- the role and where it came from, and every other role declaration;
- the native desk/project and workspace IDs with separate epic/workstream associations, or the non-desk workstream, epic and workspace chain;
- absent values by name, such as "No workspace · <cwd>", and an unknown epic
  with its reason, such as "Unknown, no epic or task label".
  A value is absent only when no display mapping or label supplies it, so a
  label that says "Other work" is shown as declared.

On a compact screen the details open over the list, from its top, and their
lines scroll when the screen is too short for them. On a wide screen they open
beside the row. Escape, a tap outside, or Close dismisses them and returns
focus to the button. The row itself still opens the session.

The details are a modal dialog. On the web, Tab and Shift+Tab cycle through
Close and the lines, which are a named stop of their own with a focus outline,
so arrows, Page Down and End scroll them. The context picker and the Board
sessions chooser keep Tab inside in the same way; menus are unchanged. A menu
or dialog that takes another's place without a close, such as the picker
reached from the open More menu, opens as it would from closed. If the
row's button has left the list by the time the details close, focus goes to
the list. Details whose session has left close on their own.

On a touch screen, session rows, Details, favorites and the Arrange handle are
44 pt at any width. A mouse keeps the denser sizes: 40 pt rows, 30 pt Details
and favorites, and a 28 × 32 pt Arrange handle.

The Role header keys (`roles/role:<code>`, `roles/no-role`) are new. Disclosure
and order saved for the former per-placement Role groups (`roles/scope:…/role:…`)
and the former Roles row (`roles`) therefore match no row, and Role headers start
closed and in the default order once. Those saved entries stay in the settings
document; nothing is migrated or deleted.

Favorites, expansion, sibling order, appearance and explicit display mappings are
stored in one native host settings document (`organization`, schema version 1).
The reorder handle accepts vertical sibling moves and opens accessible Move up /
Move down controls. Native desktop/touch behavior still needs device qualification.
Neither operation changes native placement, titles or ownership.

Settings saves use the loaded host revision. Conflicts retain the draft; reload
saved values and use **Reapply local changes to latest saved values** to review a
combined draft before saving again. Unedited fields and independent favorites,
mapping fields and associations are retained. Invalid stored preferences are
reported and preserved until an explicit reset. These are host-shared values,
not per-user settings.

Plan, Board and Graph read only configured plan sources. A Plan page renders the
actual Markdown, headings and search. Source state and time stay visible; the
expandable **Source details** holds paths, full SHA-256 and diagnostics. Board and Graph require a fresh `asma plan export` of that exact source.
The file hash is checked before and after export. Export/schema/source failures
stay visible; no ASCII, Mermaid, prose extraction or cached graph index is used.
Dependency depth is not readiness or task status. Graph integrates the separately authored
ASMA-8548 component without editing its owned files: nodes start visible; native scrolling, selection,
dependency navigation, centre-selected and the wave-list alternate are available.
Graph controls: − / + zoom, 100% Reset, Fit graph, and Fit as wave list. Pinch is not implemented; device, keyboard, text-scale and runtime qualification remains open.

Overview separates native runtime/attention from task progress, lifecycle,
decisions and delivery. The canonical export supplies task declarations but no
completion numerator. A configured historical Jira projection may supply captured
status/category counts for exact task and parent keys; current completion stays
Unknown. Missing or conflicting observations remain Unknown. Decisions require actual
structured records; missing decision data is not zero. Inbox opens native
sessions without answering or dismissing items. Activity is a partial snapshot
of session update times. Search covers available names, configured plan text and
canonical tasks, with no private transcript indexing. Delivery, Decisions and
configured team definitions/chains show their own source and coverage, or an
explicit unavailable state. Governed scheduler/gate data is not shown here; the
page states what `kontor.status` observed on this host instead. None of these
direct reads requires `kontord`.

### Host-side source catalog

`KONTOR_PLUGIN_READ_SOURCES` is a JSON array configured in the plugin server's
environment. A nonblank value takes precedence, including `[]` to select no
sources; an invalid value reports its error without falling back. When the value
is absent or blank, the server reads the operator-owned file
`$PASEO_HOME/plugin-config/kontor/read-sources.json` (default
`~/.paseo/plugin-config/kontor/read-sources.json`). This uses the existing Paseo
home configuration and the same JSON entry validation, unique IDs and maximum
of 512 entries. The file must be regular and at most 2 MiB. An absent file leaves
an honest no-sources state; malformed or unreadable files report diagnostics.

The server rereads the catalog for each list, read and association request. An
operator can atomically replace the file; Settings → Refresh sources or the
30-second source refresh picks up additions, edits and removals without a daemon
restart. A catalog changed during a refresh fails that refresh rather than mixing
bindings and replacement paths. Failed requests retain previous client reads with
a stale warning; invalid or missing catalogs return their own diagnostics and no
sources. The plugin never writes the catalog or accepts its path from a client.
Presentation settings do not store catalog paths or source contents. This file
only declares read sources; it does not activate teams or grant execution.

Each entry has a unique `id`, `title`, `kind` (`plan`, `teams`,
`delivery`, `decisions` or `jira`) and absolute `path`. Optional fields are `workstream`,
confirmed `epic`, native `projectId`, `revisionKind` and `expectedSha256`.
`revisionKind` defaults to `working copy`; `saved draft` permits a plan without an
epic key. `approved revision` requires an exact `expectedSha256`. A mismatched
hash is unavailable rather than being presented as an approved working copy.

Example catalog entry (replace the illustrative path with a configured source):

```json
[{"id":"example-plan","title":"Example plan","kind":"plan","path":"/srv/plans/example.md","revisionKind":"saved draft"}]
```

The host must have the supported `asma plan export` command. Optionally set
`KONTOR_PLUGIN_ASMA_WORKSPACE` to the absolute host-side ASMA workspace containing
the validator. Otherwise the installed CLI resolves its normal workspace. The
documentation file's directory is not assumed to contain the validator. Reads
are bounded to 2 MiB; each export has a 15-second timeout. Invalid configuration,
missing files and exporter diagnostics identify the specific source.

Offline discovery prepares candidates from 1–16 explicitly selected documentation
checkouts. It reads tracked Markdown under `_docs/`, `plan/` or `plans/`, excluding
routers, histories, reports, audits, references and adoption records. It does not
walk worktrees automatically or scan from React renders.

```sh
bun scripts/discover-plan-sources.ts /srv/asma-docs > coverage.json
bun scripts/discover-plan-sources.ts /srv/asma-docs /srv/selected-worktree --existing /srv/read-sources.json --catalog > candidate.json
```

Plans without a canonical task graph or a workspace remain discoverable. Explicit
top-level frontmatter scalars supply workstreams and `epic`, `jira_epic` or `jira`
may supply a confirmed ASMA key. Conflicting, missing, malformed, alias and nested
values stay unknown; titles never supply identity. Graph-bearing plans still use
`asma plan export` and exact path/hash validation. Graph failures stay diagnostic,
with readable Markdown and independent frontmatter retained. No graph is invented.

Every source read parses metadata from the exact file bytes after the configured
pin check. This repairs omitted catalog metadata in the read projection without
editing the catalog. A catalog/frontmatter workstream conflict disables automatic
association and appears in Source details. The registry is read separately from
`config/orchestration/workstreams.json` in `KONTOR_PLUGIN_ASMA_WORKSPACE` on each
catalog refresh. Missing registry configuration is reported; no roster is hard-coded.

The operator can configure registry resolution without changing the daemon environment:
`$PASEO_HOME/plugin-config/kontor/registry.json` (default
`~/.paseo/plugin-config/kontor/registry.json`) accepts exactly
`{"workspaceRoot":"/absolute/asma-workspace"}`. Like the catalog and `floor.json`,
this is a host-local operator file, read only by the plugin. Organization settings
and RPC inputs cannot supply the path. The plugin reads the file and registry
again on each source-list refresh; no worker or daemon restart is needed.

An explicit server-side reader argument takes precedence, followed by a nonblank
`KONTOR_PLUGIN_ASMA_WORKSPACE`, then the local file. A present invalid override
reports an unavailable registry and does not fall back. The root must be absolute;
the reader never infers it from cwd or native project names. Invalid/missing files
and unsupported registry schemas retain diagnostics and empty registration data.
Registered workstream keys and desk categories remain separate. This file configures
only registry resolution; it does not change canonical-export execution context.

Discovery deduplicates copies by repository common directory, relative plan path
and content revision. Divergent revisions remain separate source choices. Existing
entries, IDs, pins, project bindings and explicit catalog mappings are preserved;
regeneration does not repin a selected source or silently replace a user mapping.
Missing pinned paths remain unavailable until the owner repairs the path with the
same bytes or explicitly authorizes another revision. Review candidate coverage
and conflicts before the integrator installs it; the command never writes live config.

Sessions shows global registered keys, declarations in the project/epic context,
and observed native session classifications as distinct facts. Counts precede
search/state/workstream filters; zero sessions means none linked in the current
inventory. Registered desk categories are separate from workstreams. Native desk,
workspace and project identities remain native, including unlabelled sessions.

Each workstream choice names its kind first: **desk category**, **plan only** (a plan
declares it; no session or workspace is linked) or, for Other work, **no workstream
link**. Session, workspace and plan counts follow, then **registered** or
**unregistered** only while the registry is loaded; otherwise the footer says once
that it is not. Counts follow the association the choice filters by, and the tree
keeps every session of a native desk project under that desk. So a choice counting
such sessions says "N shown under DESK • …", and a desk choice says how many more
sessions its desk holds for other workstreams. Workspace counts cover workspaces
with a counted session or a workspace mapping; the tree also lists empty native
workspaces until workstream choices narrow it, and a desk choice says how many of
its desk's native workspaces are empty. The shortcut for everything outside
desks is **Outside desks** (saved views keep the `Other work` scope ID); the **Other
work** group and choice hold only sessions with no workstream association. Details
of such a session add a **Linking** line: why no plan links it (no known ASMA epic, no
configured plan for its epic, plans bound to other projects, plans that do not agree
on one ready workstream, or explicit plan links that do not resolve), and that
Settings › Presentation and associations can map it or link a plan. No project or
session name links a session.

Display precedence is: saved session mapping, saved workspace mapping, desk label,
workstream label, native desk classification, then an unambiguous declared-plan
fallback through an exact epic or explicit session-to-plan association. Explicit
plan project bindings constrain that fallback. Missing, unavailable or conflicting
plans do not attach arbitrarily. Derived mappings remain in memory and never write
preferences or native labels. Plan context includes the declaration and exact source
revision; graphless plans have no task graph or inferred lifecycle. Historical,
completed and draft plan declarations do not imply active work or live sessions.

Keep host-specific candidates and read evidence outside published source.


A projectless source can appear in several native projects through exact confirmed
epic labels on sessions in each project. An explicit source `projectId` constrains
it to that project.

A session's epic is its display mapping, else what its `epic`, `jira.epic`,
`origin.epic` and `asma.epic` labels name together with the historical Jira
projection's epic for each key those labels or its `jira.issue`, `task` or
`asma.task` label name. Every named epic must agree. When they disagree, or
nothing names one, the epic is unknown and shows as Epic unknown, never as "no
epic": no source here shows that a task has no parent. Session details name the
sources and the capture's observation time, or why the epic is unknown. A
workspace carries no labels, so a project's unmapped workspaces without sessions form an
Epic unknown group that says so beside the project's known epics. A capture
cannot see a later move in Jira, so a label and a capture that agree can both be
stale. Other label keys, project names, titles and sibling labels never assign
an epic. The tree, results, Overview counts, the context picker and project
scope all read this one resolver.
Overview, the plan picker, Plan, Board, Graph and recorded Delivery use this scope.
Source edits trigger fresh hash-checked exports; parse failures retain readable
plan text without a graph, while missing files and revision mismatches stay
unavailable. No source cache or fallback graph hides these changes.

The existing `sources.read` RPC keeps its explicit JSON-record format (including
Teams JSON records); it does not infer text/YAML:

```json
{"teams":[{"id":"example","title":"Configured example","roles":[{"role":"SWE","chain":["provider/model"]}]}]}
```

Decision records use `decisions: [{id, title, state}]`, where state is `open`,
`closed` or `unknown`; optional fields are `owner`, `detail` and `url`. Delivery
records use `delivery: [{id, title, kind, state}]`, where kind is `pull request`,
`check` or `release`; optional fields are `url` and `recordedAt`. Record content
is shown as recorded, with source revision and read time. No network adapter,
credential, mutation, Connect action or native Workspaces replacement is added.

### Opt-in Teams/chains formats

The read-only `teams.sources.read` RPC uses the same catalog, with these exact
authorized ID-to-format mappings. Each ID must already exist exactly once as
`kind: "teams"` with an explicit absolute path; the plugin supplies no paths.

| Catalog ID | Adapter format | Source meaning |
| --- | --- | --- |
| `authoring` | `fleet-authoring` | YAML v2 authoring fleet; activation not attested |
| `operator` | `fleet-operator` | YAML v1 operator-file snapshot; activation not attested |
| `roster` | `core-roster` | YAML core-team roster and declared role-catalog hash |
| `templates` | `team-templates` | JSON bundled template defaults; all versions retained |

Missing, duplicate, renamed or wrong-kind IDs disable that mapping and expose
diagnostics; the plugin never guesses a replacement ID, path or format. Other
Teams IDs keep the separate JSON-record read path. No additional environment
variable, dependency or host configuration is introduced. An empty catalog does
not read a Teams file. Reads are bounded to 2 MiB and honor the existing hash pins
and approved-revision rules.

Teams shows host configuration separately from native observations in the
selected context. Each snapshot retains its own ready/partial/unavailable state,
read time, file hash, provenance and configuration state. A file modification or
read time is not a source observation time: that time remains **Not supplied**.
Offline/service failures mark retained snapshots stale. Paths, full hashes,
provider/account provenance and diagnostics sit under **Source details**; chains,
template defaults and binding metadata expand separately to keep the page compact.
Rung order and within-rung alternatives are preserved. Authoring and operator
fleets are not merged. Exact template ID/slot candidates retain every version;
no latest version or leadership roster identity is inferred. Activation,
eligibility, running models and account availability remain unattested, and
configured bindings are never joined to native sessions by title, role or model.
Governed fleet controls are not shown here; the page states what
`kontor.status` observed on this host. Direct file reads and native sessions
remain usable without it.

`test/teams-source-adapter.test.ts` pins the four reviewed source copies and the
adapter contract. `test/teams-source-integration.test.tsx` covers the opt-in server
registration, ID-only dispatch, separate JSON format, mapping failures, partial
and stale reads, collapsed diagnostics, unresolved metadata and native content.

In Settings, explicitly link one or more configured plan IDs to a native
conversation and save. The host-scoped **Open plan** composer pill uses only this
association, not a title, epic-key guess or conversation prose. One link opens
the exact source; multiple links open a chooser. The view retains a return action
to the originating conversation. Associations are reconciled every 15 seconds;
source and empty-workspace snapshots refresh every 30 seconds. A source-service
failure marks retained data/preferences unavailable or stale and disables editing.

### Historical Jira field projection

The `jira` source reads a bounded projection of the existing Tokenomics
`consumption/data@2` epics/children. It does not read the full account/usage payload
or contact Jira. Export only explicitly selected epic keys from an already
captured file, then configure that output as a read-only catalog entry:

```bash
bun apps/paseo-plugin/scripts/export-jira-history.ts /srv/captures/consumption-data.json ASMA-8539 > /srv/exports/jira-history.json
```

```json
[{"id":"jira-history","title":"Historical Jira","kind":"jira","path":"/srv/exports/jira-history.json","revisionKind":"working copy"}]
```

This explicit offline exporter is separate from the consumption pipeline and
writes only the bounded JSON to stdout. The plugin retains its 2 MiB read cap.
The projection retains the original observation time, source hash and selected
keys; reading it later never makes it live. Exact parent/issue keys supply
historical workflow observations in Overview, Board and Graph, and the epic of a
session whose labels name a captured key. The numerator is
captured Jira `done` categories; the coverage denominator is all tasks in each
canonical plan, including missing/unbound tasks. Neither capture coverage nor a
known-child count establishes complete Jira inventory, current completion,
readiness, delivery or epic acceptance. The source's total captured epic count
is provenance, not a completeness promise. Conflicting observations stay Unknown.

### My work and assignment (responsibility projection)

Sessions opens in **My work**: every native session on the selected host,
whatever its Jira assignee, epic, task or workstream, narrowed only by the
usual search, state, archive and scope filters. It needs no person and no
projection. My work lists only groups and association choices that hold
sessions here; **All work** shows the same sessions and adds empty workspaces
and plan-only choices.

**Assignment** is a separate filter, **Any assignment** by default.
**Assigned to me** keeps the sessions where the **Working as** person is the
epic assignee, the exact task assignee, or a declared workstream responsible or
participant, each shown with its reason; **Assigned to** names one person;
**Unknown assignment** keeps the sessions whose relation could not be read. An
assignment filter hides groups with none of its sessions.

**Working as** is chosen once from the projection's roster per host and
tracker realm and remembered on this device: in the app's user defaults on
iOS, in the app's local storage on web and desktop. Where the device offers
the plugin no store (Android), or the store fails, the choice lasts the app
session and Filters says so. It holds only the tracker account ID. It is a
viewing choice, not sign-in, and nothing infers it from the host's Jira
credential, the session creator, cwd, titles, model or names. Choosing it
never turns on a filter. While the projection is loading or unavailable it is
kept, never shown as unset; **Forget on this device** removes it. A change of
Working as ends an Assigned to me or Unknown assignment **Show in tree**.
**Clear filters** returns to All work and Any assignment and keeps Working as,
the sort, layout and grouping; **Reset view** returns to My work, Any assignment,
Current order and the saved layout/grouping, keeping Working as. Layout is an
optional field in the existing version 1 host organization document. Legacy
Flat grouping reads as Flat with Desks & workstreams remembered; reading it
does not migrate the stored document. Settings retains its draft/revision
conflict flow and merges layout independently from grouping.

The facts come from one operator-owned file,
`$PASEO_HOME/plugin-config/kontor/responsibility.json`
(`kontor/responsibility@1`, 2 MiB cap), read by the `responsibility.read`
RPC. It is separate from `consumption/data@2` and from the source catalog.
A missing, unreadable, invalid or expired file, an unset Working as and a
person outside the roster leave an assignment filter **not evaluated**, with
the reason and a way back to Any assignment; My work is unaffected. They never
mean "no work". A session whose relation was not read (an epic or task outside
the selection, an assignee field not read, epic sources that disagree, or an
undeclared workstream) has **unknown assignment** and is never counted as
someone else's.

Collect it read-only from explicitly selected epics with asma-cli's Jira
credential helper (importable, or under `$ASMA_ROOT`). Credentials never
enter arguments, the output or logs, and a redirect to another origin is
refused before it is followed:

```bash
python3 apps/paseo-plugin/scripts/collect-responsibility.py ASMA-100 ASMA-200 \
  --workstreams /srv/operator/workstreams.json > /srv/exports/responsibility.json
```

The script reads each epic, its children and their sub-tasks (assignee, issue
type, parent and `ws:` labels). An absent assignee stays absent (not read) and
a null one is unassigned. The output expires after `--ttl-hours` (default 24).
Workstream responsibility exists only in the optional operator declaration;
every account in it is confirmed with a Jira user read:

```json
{"realm":"tracker.example","workstreams":[{"key":"alpha","responsible":["account-b"],"participants":[]}]}
```

Each declared workstream names at least one account, as lists of account ID
strings. Repeated members, empty rosters and an empty declaration are refused.
Undecided workstreams are left out, so they stay unknown. Without a
declaration, every workstream relation is unknown, and Assigned to me matches
only epic and task assignees. `--self-check` runs the offline check on synthetic
data.

**Sort** in Filters keeps **Current order** by default (state order, the saved
arrangement and Role order). **Last changed at**, **Created at** and **Title**
order the entire filtered Flat list, or the sessions within each Grouped group;
groups keep their identity and order.
Missing or invalid times sort last, and ties break on session ID. An explicit
sort ignores the saved session order without rewriting it, and Arrange stays
off until Current order is chosen again.

### Offline qualification without installation

`scripts/host-qualification.mjs` calls the installed host compiler and settings
store directly. It writes bundles/evidence to the chosen output directory and
settings only to a disposable temporary directory. It never connects to a daemon,
installs a plugin or creates native sessions. On macOS, run it with the installed
Paseo Electron helper in `ELECTRON_RUN_AS_NODE=1` mode, supplying the absolute plan
path, evidence directory and Paseo Resources directory. It checks both entries,
Hermes HBC96, real host revision conflicts, reload/restart persistence, invalid
stored-data preservation, unchanged floor hook registrations and the exact plan
export. Native devices, paired-client transport, pointer/touch reorder, keyboard,
text scaling and full installed-host UI behavior remain separate QA gates.

## Client authority boundary

The navigator borrows the selected host's connection through
`getPaseoClient(hostId)`. It receives no Kontor credential, reads no realm
state, and calls no `kontord` API. Client code only reads the native agent
and workspace directories, manages its own display state and calls the host's
`navigation.openAgent`. Verified canonical ASMA issue bindings can open their Jira
page in the device browser. It has no prompt, interrupt, permission, schedule,
workspace-write or workflow-write call. The floor hooks retain their behavior.
The added server handlers accept only configured source IDs and expose bounded,
read-only source data and host-local presentation associations.

Search, filters and scroll offset are in-memory view state for the current
client session. They are navigation preferences, not workflow state, and they
grant no authority. Organization values use the host settings API described above;
they never authorize a filesystem path or a workflow action.

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
- **KN-01 device acceptance (2026-10-19).** Igor accepted the mobile and desktop
  device checks for this iteration on candidate `e7006612` (test host
  `srv_tWq3jTzpOlc9`): "mobile check passed" and "desktop checked looks ok too".
  The acceptance covers the device test as performed and does not assert that
  every subcase below was individually instrumented or observed. The later
  code-only lifecycle hardening in `#353`, `#354`, `#357` and the final
  follow-up adds the refresh/dispose guards, their regression tests, the
  acceptance wording, the bounded return-control walk retry, single-flight
  subscriptions, abortable directory walks, a bounded update buffer and the
  archive-scope error handling. Those changes do not alter the accepted
  desktop/iPhone UI path; no new device pass is claimed for them. The
  code-only follow-up `364f5e83` adds the partial-read labeling and host-switch
  view restoration with focused unit tests; it also has no new device pass. This
  candidate still does not claim saved-view restoration or full B1, and KN-03
  does not substitute for the KN-01 checks. The procedure and evidence checklist are recorded below.

### Desktop and iPhone acceptance procedure

The device checks were accepted for revision `e7006612` on 2026-10-19; see the
acceptance note above. The procedure runs only in the authorized
installation/device window; browser screenshots alone do not satisfy the desktop
or iPhone check. Checklist items below stay unchecked unless the device session
recorded that specific evidence.

Record before starting:

- Paseo desktop app version and iPhone app version.
- Daemon/CLI version (`paseo --version`, `paseo status --json`).
- Plugin revision: repository commit and the installed plugin identity and
  revision (`paseo plugin ls --json`).
- Selected host `serverId`/label, `KONTOR_PLUGIN_MCP=off`, and `kontord`
  stopped or unavailable.
- Inventory count and pages read from the screen header; the footer says
  complete or partial for the last walk.
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
- [ ] A pending permission shows **Needs input** with an amber indicator and is
      included in the attention filter; after resolution/cancel it clears.
- [ ] On compact layouts the full toolbar collapses on deliberate downward
      scroll and reveals on a short reverse scroll or Search/Filters without a
      blank gap or visible row jump.
- [ ] The Filters button has a chevron, active count, 44x44 pt target and a
      separate Clear filters control (in the work line on compact).
- [ ] Missing-session and reconnect cases use controlled/disposable data only.
- [x] Desktop pass/fail, iPhone pass/fail, tester and timestamp: desktop pass,
      iPhone pass, Igor, 2026-10-19 (candidate `e7006612`, test host
      `srv_tWq3jTzpOlc9`). The remaining items were not individually recorded.

## Hooks

| Hook | Behaviour |
| --- | --- |
| `before("agent.create")` | Adds one stable system-prompt line and the `kontor-observer` MCP server at **observer** authority. A server already named `kontor` is preserved, so Kontor-composed seats are untouched. |
| `before("agent.session_open")` | Reads the session's own labels from the local daemon and sets `KONTOR_SEAT` from the keys Kontor actually writes (`kontor.seat_binding_id`, `kontor.role_slot_id`, or `kontor.agent-run`), or `KONTOR_SEAT=unbound`. A failed or slow read sets `KONTOR_SEAT=unresolved`. The key is rewritten on every open, so an old seat never survives a resume. It never blocks the open. |
| `before("workspace.create")` | Refuses a guarded seat's explicit workspace creation outside Kontor placement. A seat with any `kontor.*` label gets full binding enforcement: its bound project and canonical worktree. A Paseo-direct-only seat (`orchestration=paseo-direct` without `kontor.*`) gets only the checks that do not need a binding. Both the initial agent's labels and the caller carried on the request (`agent.callerAgentId`) are checked. Interactive and unlabelled sessions are unchanged, and an unreadable caller lookup with no seat labels on the request fails open. |

The plugin registers these three hooks unless the operator turns the floor off
with `floor.json` (see [Installation mode](#installation-mode-floorjson)).

The hooks add no menu and no agent-creation path. The navigator adds the
Kontor screen, sidebar row, the **Open Kontor** and **Back to Kontor** Command
Center commands and the per-agent **← Kontor** composer pill described above.

## Kontor placement

A seat with any `kontor.*` label may register an existing directory only inside
its own binding: the native project its own workspace belongs to and the
canonical worktree named by its `kontor.worktree` label. The binding is read
from the seat's `kontor.workspace_id` and `kontor.worktree` labels plus a
read-only Paseo workspace read.

Kontor never binds a Paseo-direct seat (`orchestration=paseo-direct` without a
`kontor.*` label). The guard still owns that seat, but applies only the checks
that need no binding. A seat that carries both label sets gets the full binding
enforcement.

Every guarded seat is refused for:

- a Paseo-owned worktree source (`kind: "worktree"`);
- a directory without an explicit `projectId` (Paseo would infer a project);
- a relative directory path;
- a directory inside Paseo's own `~/.paseo/worktrees` root;
- a directory outside `KONTOR_PLUGIN_PLACEMENT_ROOTS`, when configured.

A seat with a `kontor.*` label is additionally refused for:

- a project other than the seat binding's native project;
- a directory other than the seat binding's canonical worktree;
- an unresolved binding.

The plugin resolves the binding read-only and holds no Kontor write credential;
`kontord` still owns the native binding and readback.

## Authority

The composed `kontor-mcp` server always runs at `--credential-tier observer`.
The tier is not configurable. The plugin names the realm state root but never
reads `credentials.json`, never receives `KONTOR_AUTH`, and never writes to
`kontord`. `test/security.test.ts` pins that boundary.

The `kontor.status` RPC reports two separate facts and the time it checked.
`composition` is configuration: whether `agent.create` gives new agents the
`kontor-observer` MCP (`composed`, `mcp-off`, or `floor-off`, which wins).
`observation` is what the realm answered: `ready`, `unavailable` (no usable
answer), `refused`, or `unknown`. A missing command, credential or realm, a
start failure, a timeout or an unexpected answer is `unknown`, never "no realm"
or "not enabled". Only `kontor_realm_get`'s own 2xx answer naming a realm is
`ready`; a completed handshake is not, and `refused` is that tool's own non-2xx
answer. The probe takes exactly one `initialize` response, then one
`kontor_realm_get` response, in order. A frame that is not JSON-RPC, an error, a
duplicate, an early or unrequested response, or another tool's document ends it
as `unknown`, and nothing is sent after the answer. The child's own
notifications are skipped.

On request, at most once per 30 seconds for one configuration, the server starts
the configured `kontor-mcp` with the arguments `agent.create` composes (state
root, observer tier, serve profile) and no fallback, and calls the one observer
tool `kontor_realm_get`. The child gets only `PATH` and `HOME`, its standard
error is never read, and it is killed after the answer, 64 KiB of output
(counted in bytes before decoding), 5 seconds or the worker's stop. With `KONTOR_PLUGIN_MCP=off` nothing is started.
The RPC is registered whatever `floor.json` says, and nothing else waits for it:
Sessions, Paseo-direct work and the hooks work the same without a realm.

The Kontor screen shows the observation, the composition and the check time in
one line on every view except Sessions. Each place that names a governed
feature says it is not shown here and gives the same observation. An offline
host, an unread status or a failed read shows `unknown`, with no composition or
time. The native host status is never changed.

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
skipped. To run the navigator without the hooks, use the installation mode
below instead. Open **Kontor** from the plugin sidebar. To check the installed
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
   composer pill, the persistent sidebar entry, the global **Open Kontor**
   Command Center item or the session's **Back to Kontor** command; native back
   does not keep the plugin screen underneath
   the session. The host-scoped view store is used on return; physical
   restoration on this candidate is not claimed and remains a pending check.
4. If the host disconnects, the rows stay visible with a stale banner. Retry or
   wait for reconnect; the next subscription snapshot reconciles every page
   before the screen reports **Live** again.
5. The notices at the top (source, navigation, settings and unconfirmed
   archives) form one tray with a **Hide notices** button. Hidden notices leave
   one status button beside **Resume** (in the count row before **Grouped /
   Flat** on compact, and where the tray was on compact **Filters**) that names them and shows the tray again. Hiding runs nothing
   and resolves nothing; any new or changed notice opens the tray again, and so
   does a notice that returns after a completed read reported it gone, while one
   that stays present stays hidden. The choice is kept for the view until the
   app restarts. Escape hides the tray
   while focus is in it.

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

### Installation mode (floor.json)

The three hooks above are the floor. An operator can install the navigator
without them by writing `$PASEO_HOME/plugin-config/kontor/floor.json` (default
`~/.paseo/plugin-config/kontor/floor.json`) with owner-only permissions, then
installing or reloading the plugin:

```bash
mkdir -p ~/.paseo/plugin-config/kontor
(umask 077 && printf '{"floor":"off"}\n' > ~/.paseo/plugin-config/kontor/floor.json)
chmod 600 ~/.paseo/plugin-config/kontor/floor.json
paseo plugin reload kontor
paseo plugin logs kontor --json
```

The server reads the file once when the plugin worker starts. A supported
`paseo plugin install` or `paseo plugin reload` starts a new worker, which reads
it again; there is no live toggle and no daemon restart. The plugin never
writes, removes or watches the file, and no client, RPC or plugin setting
changes it. The daemon environment is not read for this switch.

| `floor.json` | Floor | Worker log |
| --- | --- | --- |
| absent | on | mode line, reason `not-configured` |
| exactly `{"floor":"on"}` | on | mode line, reason `explicit-on` |
| exactly `{"floor":"off"}` | **off** | mode line, reason `explicit-off` |
| anything else | on | mode line, reason `invalid`, plus the invalid-value diagnostic |

- "Exactly" is judged on the parsed JSON. Formatting whitespace around tokens
  is allowed, so a pretty-printed file is valid. Whitespace inside the value is
  not, nor is any other spelling (`OFF`, `false`, `0`), type, key, a repeated
  `floor` member, a non-regular file or a file over 2 MiB.
- A file that is present but cannot be opened, read or closed also keeps the
  floor on, with the diagnostic. A failed close counts even when the bytes read
  say off.
- When the file is off, none of `agent.create`, `agent.session_open` or
  `workspace.create` is registered. New agents get no system-prompt line and no
  `kontor-observer` server, opened sessions get no `KONTOR_SEAT`, and workspace
  creation is not guarded. The navigator, settings and read-only pages work as
  before, and existing sessions, workspaces and labels are not changed. An agent
  created while the floor was on keeps the `kontor-observer` server and prompt
  line stored in its config, so its session still starts with them when it is
  reopened while off; it just gets no `KONTOR_SEAT`.
- Off wins over `KONTOR_PLUGIN_MCP`. `KONTOR_PLUGIN_MCP=off` alone still keeps
  all three hooks.
- The mode line goes to stdout and the invalid-value diagnostic to stderr. Each
  is one JSON line, `{"event":"kontor.floor.mode","floor":"enabled"|"disabled",
  "reason":…,"file":…}` and `{"event":"kontor.floor.invalid","floor":"enabled",
  "file":…,"error":…}`. Read the installed state from `paseo plugin logs kontor
  --json` while the plugin is running. The host keeps the last 500 entries.
  The mode line reports the mode the worker started in; which hooks the host
  routes is checked separately.
- To turn the floor back on, remove the file or set it to `on`, then reload.
- Releases before this switch ignore `floor.json`, so their floor is always on.
  Rolling back to one with the file in place is not a way to keep the floor
  off. To stop the hooks on such a release, disable the plugin
  (`paseo plugin disable kontor`); its settings and native sessions stay.

## Tests

```bash
pnpm --filter @carasent/kontor-paseo-plugin typecheck
pnpm --filter @carasent/kontor-paseo-plugin test
apps/paseo-plugin/scripts/closure-suite.sh
```

`test/plan-workstreams.test.ts` reconciles every workstream choice with the tree:
desk-held sessions are named, each session counts once, and choosing a choice shows
exactly its count. It also pins choice wording and the unlinked-session plan notes.
`test/session-directory.test.ts` covers a 505-session three-page walk, repeated
and missing cursors, native upserts/removals, attention, archival scope,
recognized epic/workstream label search, disconnect staleness, reconnect
reconciliation, obsolete-generation update fencing and source failures.
`test/session-reentry.test.ts` covers returning to Kontor: live rows without a
read, updates and reconnects while closed, an unfinished first walk, stopped or
offline directories, host isolation, archive scope, cleanup, reorder-stable
removal receipts and workspace reuse.
`test/session-actions.test.ts` covers duplicate-name open by native ID and the
missing/navigation/failure cases. `test/client-entry.test.ts` pins the Kontor
screen, sidebar and the **Open Kontor** and **Back to Kontor** commands.
`test/keyboard-shortcuts.test.ts` covers the Back shortcut's key match, every
case it leaves alone, its listening phase and removal, and **Test shortcut**'s
key, Cmd-only, timeout, stop and leaving results and the keys it leaves alone;
`test/keyboard-settings-card.test.tsx` and `test/keyboard-settings.test.ts` cover
the Keyboard card states, Cancel and Copy result, and its separate settings
document. `test/return-control.test.ts`
covers per-agent pill registration, snapshot reconciliation, close/archive/
remove/workspace lifecycle, failed-bootstrap and owned-subscription-error
observation recovery, retry cleanup, ad-hoc sessions without a workspace,
late-walk fencing and opening the screen. `test/session-view-store.test.ts`
covers host-scoped query, filter, expansion and scroll restoration.
`test/session-directory.test.ts` covers the needs-input priority matrix,
permission clear/cancel and reconnect replacement. `test/sessions-screen.test.ts`
covers the filters affordance, slim toolbar actions, touch targets and saved
settings. `test/sessions-screen-events.test.ts` exercises the actual
SessionsScreen scroll/layout path for compact collapse and near-bottom reveal
compensation (including the parent gap and post-reveal range), bottom
bounce/return, top reveal and saved-settings preservation, and the compact
count row (the Filters count, no summary row, Clear filters at 44 pt in the work
line). That is code/model
evidence with analytical geometry, not a native no-jump claim.
`test/toolbar-visibility.test.ts` covers the single compact scroll/collapse
variant, pins, suppression, bounce handling and compensation helpers.
`test/security.test.ts` extends the floor checks to the client files: no Kontor
credential, no `kontord`, no file/process access and no agent-control or
workflow-write call.
`test/floor.test.ts` covers the `floor.json` value table, file kinds and sizes,
the repeated-member rule and both log lines. `test/floor-growth.test.ts` covers a
file that grows past 2 MiB after it is opened, and `test/floor-close.test.ts` a
failed read or close of the file, including a server start under a failed
close. `test/sessions-notice-tray.test.tsx` covers hiding and showing the top
notices on the wide, slim and 320/390 compact layouts: placement beside
Resume or in the compact count row, no RPC or toast on hide, persistence across a remount, reopening for a
new archive or read error, continuous presence through in-flight and failed reads,
reopening when the same notice returns after a completed read without it, focus
moves, Escape and touch targets. `test/server-entry.test.ts` starts
the server entry for each file state and checks which hooks it registers: none
when off, the same settings and RPCs as when on, `KONTOR_PLUGIN_MCP` precedence,
that `KONTOR_PLUGIN_FLOOR` has no effect, one read per start, and that the file
is never changed.

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


### Native availability and manual lifecycle actions

Attention includes unread finished results as well as current actionable notices.
A closed runtime, an old timestamp and an earlier error do not prove that a native
record is absent. Exact-ID metadata reads distinguish a readable native record,
a null record on the selected host, and an unknown/error reply. They do not load
provider history or verify that a provider conversation can continue. Reviews and
receipts keep that distinction: a row whose exact read failed is shown by its ID
and stays unknown, never missing.

The per-session X performs one exact-session Archive after native preflight.
Native archive requests cancellation and closes the runtime, including an active
turn and its provider tools. Managed child sessions observed by preflight block
this action. Only active children block, so that check reads the active
inventory and never archived history. Native identity/placement and child impact
are rechecked; activity updates alone do not make a running session impossible to stop.
While the directory has a complete live stream, a settled action changes only its
own rows; without one, the directory is read again in full.
History and files are retained. Success requires an exact-ID read showing the
session archived with no active turn; whether its runtime closed is reported
separately. A lost acknowledgement stays uncertain until
**Check native outcome** verifies retirement using metadata only. It never
resends the archive or removes the original no-replay fence.

A confirmed null native record permits only a host-local stale-row disposition,
reported separately from daemon archive. Its durable receipt is rechecked on
reload; a readable, unknown, timeout, authentication or wrong-ID reply cannot
hide a present session. Availability evidence still expires after one minute;
a settled action reconciles its exact stale list snapshot until it changes.
The action outcome remains visible when its row disappears.
The Current default is part of the combined Sessions view.

**Select** enters bulk archive selection on desktop and mobile; a row long press
also enters selection. Checkboxes support Space on web and have 44-point targets.
**Select all shown** covers only the filtered sessions. A batch contains at most
50 exact IDs and always needs one confirmation, including a one-item selection.
The confirmation keeps its session list collapsed and explains that history and
files remain, active work stops, and no undo is exposed here.
Closing the confirmation returns focus to its enabled bulk control, or the screen
while an operation is pending. On short compact screens the bulk controls and
status scroll within a bounded frame so they leave room for the sessions list.

At preflight, a parent is blocked unless every active managed child is selected.
Selected children run first; the server requires their confirmed saved archive outcomes
and rechecks native child impact before dispatching the parent. A new child
observed by that check, a blocked child or an unconfirmed child prevents a parent
archive.

Only selected session IDs are directly requested for archive. The child check
and parent archive are not atomic: a new child created between the final check
and parent archive may be archived by the host with its parent, even if unselected.
The host exposes no atomic precondition to prevent this concurrent-child cascade.

Requests remain sequential, with the existing durable per-session fences and
exact-ID readback.
**Stop before next session** lets the current request settle; it does not undo
or retry it. Pending rows stay visible, uncertain outcomes offer a metadata-only
Check, and blocked rows keep their reasons. The final status and one summary
toast distinguish archived, blocked, unconfirmed, absent and unattempted rows.
There is no whole-host bulk archive or automatic provider-history load.

Archive does not require provider history. Resume requires its separate history
and route preflight. Provider-history
existence is unverified unless an authorized operation supplies that evidence;
no bulk provider launch or permanent-death inference is performed.
