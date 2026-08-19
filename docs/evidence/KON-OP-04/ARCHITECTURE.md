# KON-OP-04 architecture handoff

Date: 2026-08-17  
Status: approved for implementation  
Scope: Project Core Team, Quick sessions, QSW-to-ESW promotion and explicit epic-roster upgrade

## Decision

Compose OP-04 behind the `ApplicationOperations` boundary fixed by OP-03. Do
not add another HTTP facade, command bus, topology model or runtime placement
path.

```text
authenticated OP-03 /v1 operation
                |
                v
      kontor-daemon Services
      |       |          |
      v       v          v
kontor-teams  store   kontor-context
state rules   state   handoff capsule
      \       |          /
       \      v         /
        OP-02 semantic topology materializer
          + exact native-id readback
```

`kontor-teams` owns the Core Team, Quick-session, promotion and roster-upgrade
rules. `kontor-store` makes their state, plans, pins and receipts survive a
restart. `kontor-daemon::Services` resolves authority and DTOs, runs one
transactional command, and supplies semantic effects through the existing OP-02
topology/container/SeatBinding path. `kontor-context` remains the sole portable
handoff representation.

The daemon must not keep an authoritative `OperationalWorkflow` only in a
`Mutex`, `OnceLock` or process-local map. The aggregate may be loaded and saved
as application state, but production replay authority remains the existing
durable command receipt plus repository transaction. A restart must resume the
same planned ids and missing effects, not construct a second QSW, MiniProject,
ESW, ECP or seat.

## Verified baseline

OP-01 supplies the data vocabulary OP-04 consumes:

- immutable topology specifications and role-catalog revisions;
- `PSW`, `QSW`, `ESW` and `ECP` as data-defined topology kinds;
- `LSA` and `TPM` as distinct role codes and SeatBindings, never node kinds;
- exactly one ECP below each ESW in the Operational default;
- typed catalog role references, server-owned code help and immutable pins.

OP-02 supplies the only accepted production placement path:

- topology-node identity and exact native binding/readback;
- capability-dispatched `native_root`, `native_child` and `session_host`
  projection;
- the adopted/read-back PSW base;
- ESW-as-native-project and ECP/QSW-as-native-child placement;
- durable SeatBinding and liveness evidence;
- `placement_blocked` before launch when parent, kind, `cwd`, identity or
  readback disagrees.

OP-03 supplies the public contract and composition boundary:

- closed DTOs, `/v1` handlers, OpenAPI operations, registry entries, CLI/MCP
  parity and minimum authority tiers;
- preview/apply, expected-revision, idempotency and receipt conventions;
- semantic topology ensure/materialize operations and topology upgrade;
- typed `Unavailable` stubs in `Services` for every successor operation.

`Services::materialize_topology` is the established pattern: resolve the
semantic scope, check the pinned specification, ensure the chain, admit seats
only on a declared session host, record a durable command and return one
authoritative projection. OP-04 reuses its lower-level seams. It must not call
the public operation recursively because the generic path currently creates one
delivery control slot, while OP-04 must materialize an exact frozen Core Team
roster.

## Service composition

### Domain and repository boundary

Persist these facts through the existing repository/store transaction style:

1. immutable project Core Team revisions and the current project revision;
2. Quick-session identity, selected resolved role, QSW node, seat binding,
   purpose, source evidence, disposition and aggregate revision;
3. promotion intent and stable planned MiniProject/ESW/ECP/seat identities;
4. the epic's frozen Core Team, topology, catalog and other configuration
   revision references;
5. immutable handoff hash, target LSA SeatBinding and delivery acknowledgement;
6. current epic-roster pin and explicit roster-upgrade receipts.

Use the existing command-receipt/idempotency mechanism at the daemon boundary.
The same key plus the same canonical intent returns the original outcome; the
same key plus changed intent is `idempotency_conflict`. Effects are reconciled
by their already-planned Kontor ids. Native names, `cwd` and runtime ids are
readback evidence and never idempotency identity.

Previews are pure reads. They may construct an in-memory plan but commit no
draft, receipt, id or aggregate. Their canonical hash covers the current source
revision, selected immutable revisions and stable semantic effects. Apply
recomputes that plan against current state, compares the hash, then freezes new
ids and the command record before the first external effect. This follows
`preview_topology_upgrade`/`apply_topology_upgrade`: the preview authorizes
effects, not a mutable cached object.

### Semantic effect adapter

The daemon-side OP-04 effect adapter has five bounded jobs:

- materialize a QSW through the OP-02 child-container path under the exact PSW
  binding, then create/reconcile its one role SeatBinding;
- create a tracker-neutral MiniProject, logical ESW and one ECP, persist their
  frozen revisions, and materialize containers through OP-02;
- create/reconcile required/default ECP SeatBindings from the frozen roster;
- deliver the canonical `HandoffCapsule` to the exact frozen LSA SeatBinding;
- archive the source only after explicit archive intent and supported exact-id
  readback.

There is deliberately no `reparent` effect. Promotion transfers work through a
handoff and leaves the QSW as its durable source.

## Successor contracts that gain behavior

Every path below is under `/v1/projects/{project_id}` and keeps the authority
tier, handler, OpenAPI operation and `ToolSpec` already fixed by OP-03.

| Contract | Behavior supplied by OP-04 |
| --- | --- |
| `GET /core-team` | Read the current immutable project Core Team revision and resolved catalog roles. Project configuration is not one epic's seat set, so `seat_binding_id` is absent in this projection. |
| `POST /core-team:preview` | Resolve typed selections against one exact catalog revision, enforce roster invariants, compare with the current project revision and return stable effects plus `preview_hash`; write nothing. |
| `POST /core-team:apply` | Revalidate the named preview and expected project revision, publish the next immutable Core Team revision and return the project projection plus one receipt. It creates no epic seat. |
| `POST /epics/{epic_id}/core-team/seats:materialize` | Read the epic's frozen roster, ensure its one ECP and materialize missing required/default seats once. The route's `epic_id` scopes any returned `seat_binding_id`. |
| `GET /quick-roles` | Derive, never store, the ordered resolved roles whose current Core Team entries have `ad_hoc_allowed=true`. |
| `POST /quick-sessions:ensure` | Validate the role, exact PSW binding and authority; plan/reconcile one QSW plus one SeatBinding; capture server-owned source evidence and return the durable session receipt. |
| `POST /quick-sessions/{quick_session_id}/promotion:preview` | Build the tracker-neutral MiniProject, frozen revision set, ESW/ECP/roster and handoff effects from the current QSW; write nothing. |
| `POST /quick-sessions/{quick_session_id}/promotion:apply` | Revalidate source revision and preview hash, freeze stable ids, create/reconcile one MiniProject/ESW/ECP/roster, deliver the handoff to LSA and return the original result on replay. |
| `POST /epics/{epic_id}/roster:upgrade-preview` | Diff the epic's pinned Core Team revision against the named published target. Report additions and policy changes without mutating the epic. |
| `POST /epics/{epic_id}/roster:upgrade-apply` | Revalidate the preview and epic revision, move the pin and materialize only newly required/default seats. Existing identities stay stable; removals are never silent retirement. |

The existing generic topology inspect/drift/ensure/materialize/retire/archive
operations remain the diagnostic and explicit lifecycle surface. OP-04 does not
add a generic create-node route.

## Core Team rules

The Project Core Team is configuration, not a `TeamRun` and not a set of
project-global live seats.

1. Resolve every selection by exact `(catalog_id, version, role_code)` from a
   current `RoleCatalogRevision`.
2. Persist the standard title and segment from the catalog reference; never
   accept them from the caller.
3. Derive stable role-slot identity from `role_code`. A
   `custom_display_name` is secondary presentation only and changes no identity,
   authority, routing, model policy, topology placement or native name.
4. Reject duplicate, unknown or non-current role codes and any raw/free-form
   role input.
5. Require distinct `LSA` and `TPM` entries with `required` epic presence.
   Insert either missing mandatory entry from the catalog; reject an attempt to
   weaken it. `SA` never satisfies `LSA`.
6. Preserve deterministic declared order after mandatory-role normalization.
7. Publish only version one or the exact next revision. A project edit changes
   no running epic.

### OP-03 DTO correction required before enabling the routes

`CoreTeamPreviewRequest` and `CoreTeamApplyRequest` currently contain
`Vec<RoleSelectionDto>`. That type correctly carries the catalog revision,
role code and optional custom label, but it cannot express the required
`required | default | on_demand` presence or `ad_hoc_allowed` policy. Deriving
those values from a display order or role code would hard-code project policy
and make `GET /quick-roles` dishonest.

Keep the existing routes and role-selection shape, but wrap it once:

```text
CoreTeamSeatSelectionDto {
  role: RoleSelectionDto,
  presence: required | default | on_demand,
  ad_hoc_allowed: bool
}
```

Use that closed type in the two Core Team request DTOs and regenerate the
OpenAPI/client/registry parity artifacts in the same change. This is a narrow
correction to the OP-03 successor contract, not a second vocabulary. The daemon
must remain `Unavailable` until the corrected request can represent the policy
it promises to persist.

## Quick-session behavior

Quick roles are a projection of the current Core Team; there is no Quick Team
aggregate.

`quick-sessions:ensure` performs these checks before the first native effect:

1. the project and current Core Team exist;
2. the catalog revision and role code resolve exactly and that Core Team entry
   is `ad_hoc_allowed`;
3. the configured PSW native project id equals the observed readback id;
4. the pinned topology specification declares QSW as the required child/session
   host shape;
5. the idempotency key is unused or names this exact canonical request.

The daemon derives source run, Context Pack, workspace, commits, tests,
decisions, evidence, remaining work and risks from authenticated/stored Kontor
state. The caller supplies the typed role selection and bounded purpose, not a
provenance document. A missing or mismatched PSW binding is
`placement_blocked`; it never creates a fallback native project.

One successful ensure owns one stable `QuickSessionId`, QSW topology-node id and
SeatBinding id. A lost acknowledgement reuses all three. A Quick session does
not create a MiniProject, TeamRun or epic phase and therefore consumes no epic
mission slot.

## Promotion transaction

Promotion is one resumable semantic transaction with ordered effects:

1. validate the active materialized QSW, caller authority and expected source
   revision;
2. resolve the current project Core Team and freeze that exact revision plus its
   catalog hash;
3. freeze the project's current topology and applicable profile/configuration
   revisions;
4. create one tracker-neutral `MiniProject` and one logical ESW under the PSW;
5. create exactly one ECP under the ESW;
6. materialize ESW as a separate native project and ECP inside that exact
   project through OP-02;
7. create distinct LSA and TPM SeatBindings plus every other required/default
   Core Team seat in the ECP; leave on-demand roles absent;
8. build one immutable `HandoffCapsule` preserving QSW source identity,
   workspace, attempted work, files, commits, tests, decisions, evidence,
   remaining work, risks and recommended continuation;
9. deliver the exact capsule to the frozen LSA SeatBinding and persist its hash
   and acknowledgement;
10. leave the QSW idle by default.

The bodyless OP-03 promotion preview therefore uses server-owned defaults: the
MiniProject title derives from the bounded Quick purpose, activation is
tracker-neutral, and source disposition is idle. OP-04 cannot activate ASMA
Epic policy because the contract carries no confirmed Jira Epic binding; OP-07
performs that activation only after connector create/link and exact readback.

Explicit source archive remains available after successful promotion through
the existing exact-node topology archive operation. Promotion must not report
archive or reparent success itself when the fixed promotion request did not
authorize either action.

The first apply freezes every generated id in durable command state before the
first effect. Retries reconcile the same MiniProject, topology nodes, native
containers, SeatBindings and handoff. Success is returned only after handoff
delivery is acknowledged; a partial failure resumes the missing suffix.

## Topology and phase implications

- PSW remains the logical parent of both the source QSW and promoted ESW. Paseo
  does not physically nest their projects.
- QSW is a native child/session host in the adopted PSW base project.
- ESW is one separate native project. ECP is its single native child/session
  host and holds all stable epic Core Team SeatBindings.
- Role codes are SeatBinding facts. No role creates a topology kind or its own
  workspace.
- Promotion creates a tracker-neutral MiniProject in the existing pre-execution
  planning lifecycle. It does not start a TeamRun, admit delivery work, activate
  ASMA policy or skip any existing lifecycle/gate transition.
- Core Team apply changes project configuration only. Promotion freezes the
  selected revision. A later project edit has no effect on the epic.
- Roster and topology pins move independently through their own explicit
  preview/apply operations. Neither upgrade silently retires a seat or changes
  native identity.
- Required/default seats materialize once. On-demand seats require a later
  authorized semantic command; their declaration is not permission to create
  them during bootstrap.
- The QSW remains durable provenance after promotion. Its idle/archive state is
  independent of the epic's lifecycle.

## Composition checkpoints

1. Correct the closed Core Team seat DTO, implement catalog resolution and
   immutable Core Team preview/apply/read persistence, then regenerate contract
   artifacts.
2. Compose Quick-role projection and repository-backed QSW ensure through the
   OP-02 materializer; prove exact PSW refusal and lost-ack replay.
3. Compose pure promotion preview and resumable apply with frozen snapshots,
   one ESW/ECP, required/default seats and exact LSA handoff.
4. Compose explicit roster preview/apply and materialize-only additions, then
   replace the OP-03 `Unavailable` stubs with projections/receipts from the same
   service.

Each checkpoint must build. Do not enable one route with an in-memory or fake
success while its durable composition is incomplete.

## Required proofs

- unknown, non-current, duplicate and Quick-ineligible role codes refuse before
  effects;
- a raw role or caller-authored standard title is rejected by the closed DTO;
- `SA` cannot satisfy mandatory `LSA`, and LSA/TPM remain distinct seats;
- custom labels do not affect slot identity, authority, placement or native
  names;
- missing/mismatched PSW readback never creates a fallback project;
- a promotion creates one MiniProject, ESW, ECP and each required/default seat
  across duplicate, lost-ack and restart retries;
- later project Core Team/topology/catalog/profile edits leave frozen epic
  snapshots unchanged;
- explicit roster upgrade adds only newly required/default seats and preserves
  existing identities;
- handoff bytes/hash and QSW source evidence reach the exact LSA seat;
- failed handoff delivery cannot report promotion success;
- the source remains idle by default and unsupported reparent is never claimed;
- ASMA activation refuses until OP-07 supplies a confirmed Jira Epic binding;
- no OP-04 path creates a TeamRun, bypasses epic lifecycle or counts persistent
  seats as active mission capacity.

## Out of scope

OP-04 does not implement Jira create/link or ASMA activation (OP-07), Advisor or
Committee behavior (OP-05), Completion compilation (OP-06), final cross-feature
surface assembly/compatibility commands (OP-08), or diagnostic UI (OP-09). It
supplies the durable application behavior those later slices compose; their
absence is not permission to invent placeholder success.
