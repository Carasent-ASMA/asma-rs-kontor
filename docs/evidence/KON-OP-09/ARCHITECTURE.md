# KON-OP-09 architecture handoff

Date: 2026-08-17  
Status: approved for implementation  
Scope: Operational diagnostic UI/UX for project configuration, topology,
Quick work, consultations, capacity and completion

## Decision

Extend the existing React console as a thin diagnostic and command client over
the OP-03 `/v1` application boundary. Do not add a browser-side domain model,
workflow engine, direct Paseo/Jira connection, second API vocabulary or new
desktop/mobile package.

```text
operator in the existing console
              |
              v
 generated OpenAPI types + KontorClient
              |
              v
 authenticated OP-03 /v1 operations
              |
              v
 kontor-daemon Services / ApplicationOperations
   |              |                |
   v              v                v
server-owned   durable state    typed receipts,
projections    and policy       refusals and cursors
```

The browser renders server-owned facts and sends closed semantic intents. It
may retain form input and the last preview/receipt needed for the current user
interaction, but none of that state is authoritative. Every apply is
revalidated by `kontord`; every visible success comes from a confirmed server
receipt; every refresh reconstructs the screen from `/v1` projections.

Add one **Project Operations** surface for Core Team, session topology, Quick
work, consultations, capacity and completion. Rename the existing Foundation
**Teams** label to **Delivery Teams** without changing its route or identity.
Delivery Teams remain reusable ticket-execution templates; they are not merged
with the Project Core Team or with consultative Advisor/Committee definitions.

Reuse the existing session room for a selected seat or run. OP-09 adds no
second transcript, terminal, polling loop or runtime-session client.

## Verified base and inherited authority

### OP-01 — vocabulary and immutable pins

OP-01 supplies the data-defined topology kinds, role catalog, controlled codes,
server-owned code-help metadata and immutable project/epic revision pins.
OP-09 displays those returned values. It never hard-codes a second role or
topology dictionary and never accepts a display label as identity.

### OP-02 — topology and exact native readback

OP-02 supplies `TopologyNodeId`, `SeatBinding`, desired projection
capabilities, exact native binding/readback and placement/liveness evidence.
The UI presents logical topology separately from native placement. It does not
infer a container from name, `cwd`, branch, ticket or visual nesting, and it
does not offer a generic create-node or raw runtime action.

### OP-03 — one public application contract

OP-03 supplies the closed DTOs, authenticated handlers, OpenAPI document,
registry parity, minimum authority tiers, revision/idempotency rules, topology
and capacity projections, code-help route and successor contracts. Generated
OpenAPI types are the console's wire vocabulary. Hand-written TypeScript may
alias or compose generated types for view ergonomics, but must not redefine
their fields or semantics.

`kontor-daemon::Services` demonstrates the pattern OP-09 must preserve:

- `topology_projection` joins persisted logical nodes to the pinned
  specification, desired capabilities, exact observed binding and
  SeatBindings before returning one cursor-consistent projection;
- `capacity_projection` counts admitted non-terminal TeamRun envelopes once,
  restores the server-owned adaptive width/streak and returns the last
  observation/refusal instead of asking a client to calculate them;
- Core Team apply replays idempotency before revision comparison, recomputes
  the named preview, persists the immutable revision and returns a receipt plus
  authoritative readback; and
- successor methods remain typed `unavailable` until their durable service is
  composed. Empty or synthetic success is forbidden.

The console therefore has no store, policy or runtime seam beneath
`KontorClient`. It treats `unavailable`, revision conflict, placement refusal
and failed gates as first-class diagnostic results.

### OP-04 — Core Team, Quick sessions and promotion

OP-04 supplies immutable project Core Team revisions, catalog-resolved Quick
roles, QSW materialization, QSW-to-ESW promotion, frozen epic roster pins and
explicit roster upgrade. The UI sends exact catalog revision plus `role_code`,
presence, `ad_hoc_allowed` and optional custom seat label. It never submits the
standard title or segment and never treats the custom label as identity.

### OP-05 — Advisors and Committees

OP-05 supplies versioned Advisor profiles, versioned Committee templates,
frozen ASW/CSW runs, read-only seat authority, findings, dispositions,
protocol, member cardinality, rounds, verdicts and `NEEDS_HUMAN` evidence. The
UI keeps Advisors and Committees in separate subsections and renders only the
typed definition/run fields the server projects. It does not decode opaque
profile JSON or derive a protocol from a preset name or finding count.

### OP-06 — Completion

OP-06 supplies versioned Completion Profiles and one durable epic completion
aggregate compiled over existing task, TeamRun, Committee and evidence
machinery. The UI renders its typed phase, stages, blockers, round lineage,
closeout receipts, wake state and attention payload. It never turns a passing
Committee verdict into `done`, manufactures evidence or advances a phase
locally.

### OP-07 and OP-08 — connectors and client parity

OP-07 owns native Jira create/link/readback and project/subject authority
cutover. OP-09 may display confirmed bindings, conflicts and receipts already
present in Kontor projections, but never calls Jira or AgentsRoom.

OP-08 completes the shared `/v1` registry and CLI/MCP composition. The console
continues to call the same semantic paths and generated schemas; it does not
gain a privileged UI-only route. Cross-client revision/cursor differences are
shown or refreshed, never resolved by browser policy.

## Information architecture

### Navigation and concept boundaries

Keep the current shell and view-selection mechanism. The navigation exposes:

- **Project Operations** — project/epic configuration, topology, Quick work,
  Advisory and Completion;
- **Delivery Teams** — reusable ticket delivery templates and their role
  slots; and
- the existing runtime/session diagnostics, reached from selected returned
  identities.

Within Project Operations, use explicit section headings rather than a generic
"teams" grouping:

1. Project capacity;
2. Project Session Topology;
3. Project Core Team;
4. Quick sessions and Promote to Epic;
5. Advisory — Advisors;
6. Advisory — Committees; and
7. Completion Profiles and current epic completion.

Each section states what its records are for. In particular:

- Core Team is versioned project configuration whose required/default seats
  materialize into an epic ECP;
- Delivery Teams are reusable ticket-execution templates whose concrete seats
  belong to a TeamRun in a TSW; and
- Advisors/Committees are read-only consultative definitions and runs in
  ASW/CSW workspaces.

No shared card, count or action may imply that those three concepts have one
lifecycle.

### Independent projection loading

Load each section independently. One OP-05/06 `unavailable`, authorization
refusal or stale contract must not suppress already-valid topology, Core Team
or capacity evidence. Each panel has an explicit loading, ready, refused or
failed result and preserves the server's error code/message.

This is diagnostic resilience, not a client-side aggregate. The panels may
display different returned revisions/cursors and offer a full refresh. They do
not merge those facts into a synthetic project revision or claim an atomic
snapshot the server did not return.

## Contracts that gain browser behavior

All paths are below `/v1/projects/{project_id}` unless shown otherwise. OP-09
does not add or change server routes; it binds the established contracts to the
existing console.

| Contract | OP-09 behavior |
| --- | --- |
| `GET /epics/{epic_id}` | Show the epic identity and frozen topology, Team, Core Team, role-catalog, Advisory and Completion references exposed by the server. Missing pins remain visibly absent. |
| `GET /topology:inspect?epic_id=...` | Render the logical tree, each node's lifecycle/placement, desired capability, exact observed binding and SeatBindings. Never call drift implicitly. |
| `POST /topology:drift` and existing exact-node lifecycle operations | Present only as explicit operator actions where the current console already supports them; wait for and retain their receipt/refusal. No raw native action is exposed. |
| `GET /core-team` | Show ordered resolved roles, standard title, visible code, optional custom label, presence and ad-hoc eligibility with revision/cursor. |
| `POST /core-team:preview` / `POST /core-team:apply` | Provide one catalog-backed New Seat/New Role editor grouped by server segment. Preview displays normalized roster/effects/violations; apply uses that exact hash and current expected revision. |
| `POST /epics/{epic_id}/core-team/seats:materialize` | Explicitly materialize the frozen epic roster; never imply that project Core Team apply created a live seat. |
| `GET /quick-roles` | Populate Quick-role selection from the server projection only. An absent role cannot be made eligible in the client. |
| `POST /quick-sessions:ensure` | Submit exact role selection plus bounded purpose, lock the action in flight and display returned QSW/topology/role identity and receipt. |
| `POST /quick-sessions/{quick_session_id}/promotion:preview` / `:apply` | Show the server-planned tracker-neutral MiniProject, ESW, ECP, roster and handoff effects; apply only the named preview and display the confirmed outcome. |
| `GET /advisor-profiles` and profile preview/apply | List immutable revisions and expose typed Admin configuration only when the server schema can render the complete profile definition and violations. |
| `POST /epics/{epic_id}/advisor-runs:invoke` / `POST /advisor-runs/{advisor_run_id}:settle` | Show the selected profile, scope, one-ASW/one-seat lifecycle, immutable advice and recorded disposition from returned typed state. |
| `GET /committee-templates` and template preview/apply | Show immutable revisions with server-projected member count, ordered slots, provider-diversity rule, protocol, verdict rule and round budget. Never infer them from `independent_review@1`. |
| Committee invoke/findings/settle contracts | Show one CSW, frozen seats, immutable findings, dissent, round and aggregate state. A Judge result before required findings remains a refusal/pending state. |
| `GET /completion-profiles` and profile preview/apply | Show immutable typed Completion Profile definitions and compilation violations; apply creates configuration only. |
| `GET /epics/{epic_id}/completion` | Render the exact typed phase/stage, stable blockers, integration/round lineage, closeout receipts, wake and attention state. |
| `POST /epics/{epic_id}/completion:advance` / `:remediate` | Send only the closed semantic request at the current revision; display the returned state and receipt. Never submit caller-authored evidence or verdicts. |
| `GET /capacity` | Display Active TeamRun count, MiniProject Concurrency Ceiling, Adaptive Admission Window, clean-observation streak, last observation and last refusal as distinct server values. |
| `GET /epics/{epic_id}/code-help` | Load the one sorted server dictionary used by every controlled-code rendering on the page. |

The OP-04 `CoreTeamSeatSelectionDto` correction is a prerequisite for the
Core Team editor because presence and `ad_hoc_allowed` cannot be inferred. The
OP-05 profile/run and OP-06 completion DTO corrections described in their
architecture handoffs are likewise prerequisites for their complete panels.
Until those generated fields exist, the panel displays the typed
`unavailable`/missing-contract state. It must not parse an opaque definition,
interpret free-form lifecycle strings or silently omit member count, protocol,
round, blockers or closeout evidence.

## Mutation and consistency rules

Every console mutation follows one of two established shapes:

```text
direct semantic command
  form -> one idempotency key -> request -> confirmed receipt/refusal

preview/apply command
  form -> pure preview -> preview_hash + expected revision
       -> apply with one idempotency key -> confirmed receipt/readback
```

Rules:

1. Disable the initiating control while its request is in flight. A double
   click or touch cannot create a second browser intent.
2. Reuse the same idempotency key when retrying one uncertain request. Editing
   the intent creates a new key and invalidates its prior preview.
3. Do not optimistically change authoritative state. Keep the form/preview
   visible until the server returns a receipt or refusal.
4. After success, display `receipt_id`, `applied`, aggregate revision and
   snapshot cursor, then refresh the affected projections.
5. Keep revision conflict, idempotency conflict, `placement_blocked`, failed
   gate, `capacity_blocked`, `unavailable` and `NEEDS_HUMAN` visible with the
   server's message and applicable code help.
6. Never convert a refused or transport-unknown response into success. A
   transport retry remains the same intent.

The last preview and receipt are interaction evidence, not a browser event
store. A page reload may lose them without losing any authoritative operation;
the refreshed server projection remains decisive.

## Topology presentation

Present two explicitly labelled relationships:

```text
Kontor logical lineage                  Paseo native placement

PSW                                    adopted PSW base project
|- QSW                                 `- QSW workspace/session host
`- ESW                                 separate epic project
   |- ECP                              |- ECP workspace
   |  |- LSA SeatBinding               |  |- LSA session
   |  `- TPM SeatBinding               |  `- TPM session
   |- TSW                              |- TSW workspace + delivery seats
   |- ASW                              |- ASW workspace + Advisor seat
   `- CSW                              `- CSW workspace + Committee seats
```

The logical PSW-to-ESW edge must not be drawn as native project nesting. Each
ESW is a separate native Paseo project. ECP, TSW, ASW and CSW are ordinary
workspaces inside that exact epic project. ECP is one workspace containing the
distinct LSA, TPM and authorized optional epic SeatBindings; it is not a
folder of role workspaces. QSW remains under the adopted PSW base and stays
durable source provenance after promotion.

For every node, keep the controlled kind code visible and render logical id,
parent, lifecycle, desired capability, observed runtime/native id, `cwd` and
readback time only when returned. A missing or mismatched observed binding is
diagnostic evidence, not a reason to rearrange the tree or infer adoption.

## Phase implications

- Core Team preview/apply publishes project configuration. It creates no ECP
  seat and changes no running epic; materialization is explicit.
- A Quick session creates one QSW and one eligible seat. It creates no Task,
  TeamRun or epic phase and consumes no mission slot.
- Promotion creates a tracker-neutral MiniProject, logical ESW, one ECP,
  frozen roster and LSA handoff. It does not start delivery, activate ASMA Jira
  policy or reparent/archive the QSW.
- Delivery Team selection and TeamRuns remain ticket-scoped. Their active
  non-terminal envelopes, not their seats, contribute to capacity.
- Advisor and Committee runs are read-only consultation evidence. ASW/CSW are
  not Tasks/TeamRuns and their idle seats do not contribute to mission count.
- Completion is epic-local state, not a topology node. It consumes existing
  Task/TeamRun/Committee/evidence receipts and advances only through the
  server's typed transitions.
- Jira materialization/activation is an OP-07 server concern. The UI never
  treats a typed key string as confirmation and never connects to Jira.
- A panel may remain unavailable while another is ready. Dependency rollout
  changes presentation completeness, not the authority boundary.

## Shared accessible code help

Render every controlled role, topology, lifecycle, phase, protocol and refusal
code through one shared `CodeHelp` component fed by the epic's
`CodeHelpProjectionDto`.

- The compact code text remains visible in normal layout.
- A real focusable button associates the code with its full name and meaning;
  pointer hover, keyboard focus and click/touch expose the same content.
- Category participates in lookup so equal text in different vocabularies
  cannot select the wrong definition.
- Tooltip/popover content comes only from the server entry and is associated
  accessibly; a visual `title` attribute alone is insufficient.
- If no entry matches, keep the original code visible and state that it is an
  unknown code. Do not title-case, expand or guess it locally.
- Loading code help once per project/epic view is sufficient. Individual code
  components perform no network request.

Codes that are identifiers rather than controlled vocabulary remain ordinary
selectable text; the component is not a generic glossary lookup.

## Responsive and keyboard behavior

Use the existing console layout, controls and CSS. At desktop width, cards may
use the available columns. At phone width, preserve the same sections and
document order in one column; dense topology/tabular evidence may scroll
horizontally inside its own labelled region but no field or action disappears.

All forms have persistent labels, native keyboard order and visible focus.
Preview, Apply, Quick Session, Promote, consultation and completion actions are
reachable without a pointer. Code-help disclosure, refusals and receipts are
reachable and dismissible by keyboard and touch. Color is never the sole
state/refusal indicator. The existing session room remains the only detailed
run transcript surface at both widths.

## Component boundary

The minimum implementation shape is:

1. extend `KontorClient` with typed methods for the existing Operational
   routes and keep authentication, error decoding and idempotency handling in
   that one transport class;
2. alias generated OpenAPI DTOs in the API type module instead of duplicating
   interfaces;
3. add one Project Operations view that independently loads and renders its
   sections;
4. add one shared `CodeHelp` component;
5. add the Project Operations navigation entry and rename the user-facing
   Teams label to Delivery Teams; and
6. reuse existing form, table, status, shell and session-room patterns.

Do not add a state-management dependency, route framework, design-system fork,
generic resource client, plugin layer or speculative abstraction for future
Operational modules.

## Required proofs

- generated-client drift fails verification when an Operational route or DTO
  changes;
- Core Team selection uses exact catalog revision/role code, groups roles by
  server segment, keeps standard title/code visible and treats custom label as
  presentation only;
- presence and ad-hoc eligibility are explicit and no Quick-ineligible role is
  locally offered;
- apply cannot run without its matching preview/revision where the server
  contract requires preview/apply;
- rapid double activation sends one request while in flight, and replay uses
  the original idempotency key;
- success is rendered only from a server receipt; revision, placement, gate,
  capacity and unavailable refusals remain visible;
- logical PSW-to-ESW lineage is visually distinct from native Paseo placement,
  and ECP/TSW/ASW/CSW do not appear as nested native projects;
- Core Team, Delivery Teams, Advisors and Committees retain distinct labels,
  identities and lifecycles;
- Committee member count/protocol and Completion phase/blockers come from typed
  server fields, never preset-name or string heuristics;
- active TeamRun total, ceiling, adaptive width, streak and last refusal are
  displayed independently with no client calculation;
- every controlled code exposes server help through hover, focus and touch;
  an unknown code stays visibly unknown;
- keyboard-only desktop and phone-width paths reach the same data and actions;
- a failed panel does not erase successful sibling projections; and
- network/dependency scans find no console connection to Paseo, Jira,
  AgentsRoom state or a non-`/v1` authority source.

## Verification boundary

Use component/contract tests for independent loading, exact generated request
shapes, catalog grouping, custom-label display, mutation locking, preview/apply
revision flow, receipts and refusals. Test `CodeHelp` for known and unknown
codes plus hover, focus and click/touch disclosure. Use responsive browser
checks and committed desktop/phone screenshots for topology, forms and
diagnostic state. Keep daemon build/API-generation verification in the gate so
the UI cannot quietly bind a stale successor contract.

## Out of scope

OP-09 does not implement or repair OP-04/05/06/07 application services, add a
server route, mutate topology directly, create Jira records, derive capacity or
lifecycle, edit Delivery Team semantics, add a workflow/profile designer,
replace the session room, introduce a separate mobile application or claim
success for an unavailable successor projection. OP-10 owns the integrated
live Operational proof and OP-11 owns mutation/security closeout.
