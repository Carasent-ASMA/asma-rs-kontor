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
- **Search and filters.** Search matches every token against the full original
  session title, workspace name, project name/key, cwd and recognized
  structured labels (`epic`, `workstream`, PR and ticket identifiers). Native
  state and attention filter independently; attention includes **Needs input**
  permission waits. The advanced area shows an active-filter count and a clear
  action. On compact layouts the Filters button has a visible down/up chevron,
  an active-count badge and a 44x44 pt target; Clear filters is a separate
  control.
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

## Organization and read-only pages

The plugin body has Sessions, Overview, Plan, Board and More on compact clients.
Desktop also exposes Graph and Delivery directly. More contains the remaining
destinations, including Inbox, Search and Settings. The compact context picker
offers All projects, observed native projects, confirmed epic keys and configured
plans. The same picker scopes Overview; All projects clears the selection. Plan queries and positions are scoped to the host,
project, epic, exact source identity and revision. Organization placement uses
explicit display mapping, then desk, then workstream. Back retains the previous query, position and session
filters; the native **← Kontor** control returns to the last host-local view.

Sessions supports **Desks & workstreams**, **Desks & workspaces**, Role and Flat
views. Native durable desks use the host's project ID, then its actual workspace
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

The workstream association choices list only values resolved from sessions in the
current scope: session mapping, workspace mapping, `desk` label, `workstream`
label, native DESK project fallback, then Other work, in that order. These choices
are neither a native project inventory nor a complete registry of workstreams.
Native projects/workspaces have their own context scope and native IDs. Sessions
of non-desk projects without declarations remain under Other work and their
labelled epic, or No linked epic; no workstream is inferred from their titles.

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
- absent values by name, such as "No linked epic" or "No workspace · <cwd>".
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
explicit unavailable state. Governed scheduler/gate data says **kontord is not enabled**. None of these direct reads requires `kontord`.

### Host-side source catalog

`KONTOR_PLUGIN_READ_SOURCES` is a JSON array configured in the plugin server's
environment. A nonblank value takes precedence, including `[]` to select no
sources; an invalid value reports its error without falling back. When the value
is absent or blank, the server reads the operator-owned file
`$PASEO_HOME/plugin-config/kontor/read-sources.json` (default
`~/.paseo/plugin-config/kontor/read-sources.json`). This uses the existing Paseo
home configuration and the same JSON entry validation, unique IDs and maximum
of 64 entries. The file must be regular and at most 2 MiB. An absent file leaves
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

Offline discovery can prepare a candidate from one explicitly selected documentation
checkout (no network access, installation or checkout changes):

```sh
bun scripts/discover-plan-sources.ts /srv/asma-docs > coverage.json
bun scripts/discover-plan-sources.ts /srv/asma-docs --catalog > candidate.json
```

Discovery examines tracked Markdown with a `plan-graph` block and validates each
exact file through the same exporter and hash checks as the RPC. Only canonical
ASMA epic bindings become catalog entries. Unbound plans, parse failures, hashes,
task binding coverage and multiple plans per epic stay in the report; multiple
plans retain an explicit choice. The report identifies the checkout HEAD and
working-copy hashes. It does not choose between divergent checkouts, supersede an
orchestration plan pin, prove approval or infer an epic from a title. Review the
candidate against the owning plan pins before installation. Paths in this output
belong to that host; keep live candidates and coverage outside published source.

A projectless source can appear in several native projects through exact confirmed
epic labels on sessions in each project. An explicit source `projectId` constrains
it to that project. Conflicting epic labels leave a session unattributed. No
project name, title or sibling label assigns an epic to an unlabelled session.
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
Governed fleet controls remain unavailable: **kontord is not enabled**. Direct
file reads and native sessions remain usable without it.

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
historical workflow observations in Overview, Board and Graph. The numerator is
captured Jira `done` categories; the coverage denominator is all tasks in each
canonical plan, including missing/unbound tasks. Neither capture coverage nor a
known-child count establishes complete Jira inventory, current completion,
readiness, delivery or epic acceptance. The source's total captured epic count
is provenance, not a completeness promise. Conflicting observations stay Unknown.

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
      separate Clear filters control.
- [ ] Missing-session and reconnect cases use controlled/disposable data only.
- [x] Desktop pass/fail, iPhone pass/fail, tester and timestamp: desktop pass,
      iPhone pass, Igor, 2026-10-19 (candidate `e7006612`, test host
      `srv_tWq3jTzpOlc9`). The remaining items were not individually recorded.

## Hooks

| Hook | Behaviour |
| --- | --- |
| `before("agent.create")` | Adds one stable system-prompt line and the `kontor-observer` MCP server at **observer** authority. A server already named `kontor` is preserved, so Kontor-composed seats are untouched. |
| `before("agent.session_open")` | Reads the session's own labels from the local daemon and sets `KONTOR_SEAT` from the keys Kontor actually writes (`kontor.seat_binding_id`, `kontor.role_slot_id`, or `kontor.agent-run`), or `KONTOR_SEAT=unbound`. A failed or slow read sets `KONTOR_SEAT=unresolved`. The key is rewritten on every open, so an old seat never survives a resume. It never blocks the open. |
| `before("workspace.create")` | Refuses a labelled Kontor seat's explicit workspace creation outside that seat's own placement: its bound project and canonical worktree. Both the initial agent's labels and the caller carried on the request (`agent.callerAgentId`) are checked. Interactive and unlabelled creation is unchanged, and an unreadable caller lookup with no seat labels on the request fails open. |

The plugin registers these three hooks unless the operator turns the floor off
with `floor.json` (see [Installation mode](#installation-mode-floorjson)).

The hooks add no menu and no agent-creation path. The navigator adds the
Kontor screen, sidebar row, the **Open Kontor** and **Back to Kontor** Command
Center commands and the per-agent **← Kontor** composer pill described above.

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

`test/session-directory.test.ts` covers a 505-session three-page walk, repeated
and missing cursors, native upserts/removals, attention, archival scope,
recognized epic/workstream label search, disconnect staleness, reconnect
reconciliation, obsolete-generation update fencing and source failures.
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
bounce/return, top reveal and saved-settings preservation. That is code/model
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
close. `test/server-entry.test.ts` starts
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
