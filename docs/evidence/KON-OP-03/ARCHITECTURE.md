# KON-OP-03 architecture handoff

## Decision

Extend the existing `ApplicationOperations` boundary. Do not add an Operational
HTTP facade, a second command bus, or policy in the MCP/CLI clients.

```text
authenticated /v1 route
        |
        v
ApplicationOperations + closed DTOs
        |
        v
kontor-daemon Services
   |          |             |
   v          v             v
topology   kontor-accounts  scheduler/runtime connectors
store      capacity policy  exact native readback
```

The route, OpenAPI operation, `ToolSpec`, generated CLI command and MCP tool are
one operation. `kontor-mcp::registry::REGISTRY` remains the shared client
vocabulary and carries the minimum caller tier. The clients validate and
transport; only `kontord` resolves policy and performs effects.

The model-facing topology boundary is semantic. It may ensure an epic control
plane, ticket, Quick session, Advisor or Committee scope, and it may address an
already-returned topology-node id for retirement. It may not supply a node kind,
parent, native name, native id, `cwd`, provider threshold, process id or argv.

## Verified baseline

OP-01 already supplies the reusable domain and repository seams:

- immutable `ProjectSessionTopologySpec` and `RoleCatalogRevision` documents;
- project topology defaults and epic `TopologySnapshot` pins containing
  `(spec_id, version, canonical_hash)`;
- generic topology nodes, logical `SeatBinding`s, native-container bindings and
  persisted `AdaptiveAdmissionState`;
- code-help metadata on topology kinds and catalog roles.

OP-02 already routes production admission through topology-node placement,
materializes containers by declared projection capability, records seat
liveness and refuses invalid placement before launch. Reuse that path for every
semantic ensure/materialize command; a public route must not grow a second
materializer.

Commit `5f95fa1` closes the Foundation readback defect: `epic-get` now returns
the Team revision frozen by `epics:apply`, and the black-box test proves the same
value immediately and after reopening the Realm state root.

The remaining baseline is intentionally not called Operational:

- topology publication, catalog lookup, code help and semantic topology actions
  have no `/v1` application operations;
- `DEFAULT_CAPACITY` still has mission/adaptive ceilings of eight and
  `Services::snapshot` starts a fresh adaptive window on every plan;
- the persisted adaptive state is not part of the production scheduling path;
- `kontor-accounts` still says `asma fleet` owns cooldown mechanics;
- `kontor-integrations-asma::fleet` still invokes `AsmaExecutable` for fleet
  preflight/status/block;
- `/v1/commands/{kind}` is still an explained registry omission even though the
  Operational contract permits non-agent omissions only for health, OpenAPI and
  process probes.

## Uniform `/v1` contract

### Authority and mutation rules

| Tier | Permitted Operational surface |
| --- | --- |
| Observer | Deterministic projections, status, catalog lookup, code help and stored evidence. No writes or live refresh. |
| Operator | Semantic workflow effects, topology observation/materialization, capacity refresh/override and exact-seat attention/retirement. |
| Admin | Everything above plus topology-specification, project-configuration and capacity-configuration publication or upgrade. |

Every mutation takes exactly one caller-supplied `Idempotency-Key` header and an
`expected_revision` in its JSON body. A repeated key with the same canonical
intent returns the original receipt; a changed intent returns
`idempotency_conflict`; a stale aggregate returns `revision_conflict` with no
effect. A preview is a read, takes no idempotency key and returns a
`preview_hash`; the corresponding apply names that hash and revalidates the
current state before its first effect.

Keep the existing `ApiErrorBody`. Add specific static rules, but add a new error
code only when no existing code is truthful. Unsupported successor-ticket
contracts use `unavailable`, never a successful placeholder projection.

Every new mutation answer carries at least `receipt_id`, `applied`, the affected
aggregate revision and `snapshot_cursor`. Every new read projection carries
`realm_id`, its resource identity, aggregate revision and `snapshot_cursor`.
Topology-bearing projections additionally carry:

```text
pinned_spec: { id, version, canonical_hash }
nodes[]:
  topology_node_id
  parent_topology_node_id?
  kind_key
  lifecycle
  placement
  desired_binding       # server-derived capability/native shape
  observed_binding?     # exact runtime kind/native ids/cwd/readback instant
  seats[]               # typed role reference and exact SeatBinding identity
```

These fields are readable evidence. Their presence in a response does not make
them legal request fields.

### Topology specification, catalog and reference

| Tool | Method and path | Tier | Contract |
| --- | --- | --- | --- |
| `kontor_topology_spec_draft` | `POST /v1/projects/{project_id}/topology-specs:draft` | Admin | Pure server builder. Accepts the data-defined vocabulary plus optional base revision; returns a complete candidate and `candidate_hash`. Persists nothing. |
| `kontor_topology_spec_validate` | `POST /v1/projects/{project_id}/topology-specs:validate` | Admin | Validates and canonicalizes one complete candidate; returns ordered violations and `validation_hash`. Persists nothing. |
| `kontor_topology_spec_publish` | `POST /v1/projects/{project_id}/topology-specs:publish` | Admin | Publishes an immutable candidate after revalidation; body includes `validation_hash` and `expected_revision`. Returns spec id/version/hash, shareability and receipt. |
| `kontor_topology_spec_get` | `GET /v1/projects/{project_id}/topology-specs/{spec_id}/{version}` | Admin | Exact immutable document, canonical hash and shareability. |
| `kontor_role_catalog_get` | `GET /v1/catalog/role-catalogs/{catalog_id}/{version}` | Observer | Deterministic catalog revision, sorted in its declared order. |
| `kontor_role_get` | `GET /v1/catalog/role-catalogs/{catalog_id}/{version}/roles/{role_code}` | Observer | One resolved catalog entry; unknown revision/code is `not_found`, never guessed. |
| `kontor_code_help_get` | `GET /v1/projects/{project_id}/epics/{epic_id}/code-help` | Observer | One combined, sorted projection from the epic's pinned topology spec, pinned role catalog and shared system/planning definitions. |

Draft is deliberately a pure operation. OP-01 has no durable draft aggregate,
and publication already revalidates the exact candidate. Adding another store
solely to remember editor scratch state would not improve the authority boundary.

Use one typed role shape everywhere a new Core Team or Delivery Team seat is
selected:

```text
RoleSelection {
  catalog_revision: { id, version },
  role_code,
  custom_display_name?
}

ResolvedRoleRef {
  catalog_revision: { id, version },
  role_code,
  standard_title,
  segment,
  custom_display_name?
}
```

Requests do not contain `standard_title`, `segment` or a free-form `role`.
`kontord` resolves those facts and every projection/receipt returns the complete
`ResolvedRoleRef`. Change the existing Delivery Team draft slot DTO from raw
role JSON to this selection; do not add a second Delivery Team endpoint.

`CodeHelpEntryDto` is keyed by `(category, code)` and contains exactly `code`,
`full_name`, `meaning`, `category`, `lifecycle` and a source revision. Duplicate
keys or empty text refuse projection construction. Compatibility and retired
codes remain explicit entries; an unknown code is rendered by the client as
unknown because the server returned no definition.

### Semantic topology

| Tool | Method and path | Tier | Contract |
| --- | --- | --- | --- |
| `kontor_topology_inspect` | `GET /v1/projects/{project_id}/topology:inspect` | Observer | Stored authoritative tree; optional `epic_id` query narrows to one pinned subgraph. No runtime call. |
| `kontor_topology_drift` | `POST /v1/projects/{project_id}/topology:drift` | Operator | Exact-id native readback for a semantic scope, persisted as observation evidence; returns the updated projection and receipt. |
| `kontor_topology_ensure` | `POST /v1/projects/{project_id}/topology:ensure` | Operator | Ensures logical nodes for one `SemanticTopologyTarget`; no native effect. |
| `kontor_topology_materialize` | `POST /v1/projects/{project_id}/topology:materialize` | Operator | Materializes/reconciles the ensured target through OP-02's capability-dispatched path and exact native parent binding. |
| `kontor_topology_retire` | `POST /v1/projects/{project_id}/topology/nodes/{topology_node_id}:retire` | Operator | Retires one returned node after child/seat policy checks; no caller-authored native action. |
| `kontor_topology_archive` | `POST /v1/projects/{project_id}/topology/nodes/{topology_node_id}:archive` | Operator | Archives one already-retired node after exact readback and declared archive policy. |
| `kontor_topology_upgrade_preview` | `POST /v1/projects/{project_id}/epics/{epic_id}/topology:upgrade-preview` | Admin | Diffs the pinned spec against one published target and reports node/seat/native effects. |
| `kontor_topology_upgrade_apply` | `POST /v1/projects/{project_id}/epics/{epic_id}/topology:upgrade-apply` | Admin | Applies the named preview under expected epic revision, then returns the new immutable pin and topology projection. |

`SemanticTopologyTarget` is a closed tagged union: project root, Quick session,
epic, epic control, ticket, Advisor consultation or Committee consultation. Its
fields are the semantic ids already owned by Kontor. Adding a published kind to
a specification therefore needs no API change, while inventing a kind per call
remains impossible.

Inspect is a stored read. Drift is a write because it records the raw readback
before deriving disagreement. Ensure creates logical state only; materialize is
the only route in this family allowed to reach a runtime. Neither route accepts
native placement.

### Successor-ticket contracts

OP-04, OP-05 and OP-06 own the application behavior below, but they do not own a
competing wire vocabulary. OP-03 adds the closed DTO, handler,
`ApplicationOperations` method, OpenAPI operation and `ToolSpec` now. Until the
owning service is composed, the daemon returns typed `unavailable` before any
effect. It must not return fake empty success and it must not persist placeholder
aggregates.

| Family | `/v1` operations fixed by OP-03 | Minimum tier |
| --- | --- | --- |
| Core Team | `GET /projects/{project_id}/core-team`; `POST /core-team:preview`; `POST /core-team:apply`; `POST /epics/{epic_id}/core-team/seats:materialize` | Observer read; Admin configuration; Operator materialization |
| Quick work | `GET /projects/{project_id}/quick-roles`; `POST /quick-sessions:ensure` | Observer; Operator |
| Promotion | `POST /quick-sessions/{quick_session_id}/promotion:preview`; `POST /quick-sessions/{quick_session_id}/promotion:apply` | Operator |
| Epic roster | `POST /epics/{epic_id}/roster:upgrade-preview`; `POST /epics/{epic_id}/roster:upgrade-apply` | Admin |
| Epic | Keep `/projects/{project_id}/epics:apply` and `/epics/{epic_id}`; the read must return the frozen Team, topology, Core Team, role-catalog, Advisory and Completion revision refs as they become available. | Admin apply; Observer read |
| Delivery Team | Keep `/tasks/{task_id}/team-selection`; replace its body with an exact template revision and typed role selections where seats are chosen. | Admin |
| Advisor configuration | `GET /advisor-profiles`; `POST /advisor-profiles:preview`; `POST /advisor-profiles:apply` | Observer; Admin writes |
| Advisor run | `POST /epics/{epic_id}/advisor-runs:invoke`; `POST /advisor-runs/{advisor_run_id}:settle` | Operator plus server-side seat authority |
| Committee configuration | `GET /committee-templates`; `POST /committee-templates:preview`; `POST /committee-templates:apply` | Observer; Admin writes |
| Committee run | `POST /epics/{epic_id}/committee-runs:invoke`; `POST /committee-runs/{committee_run_id}/findings:record`; `POST /committee-runs/{committee_run_id}:settle` | Operator plus server-side seat authority |
| Completion configuration | `GET /completion-profiles`; `POST /completion-profiles:preview`; `POST /completion-profiles:apply` | Observer; Admin writes |
| Completion run | `GET /epics/{epic_id}/completion`; `POST /epics/{epic_id}/completion:advance`; `POST /epics/{epic_id}/completion:remediate` | Observer; Operator writes |

Every path in the table is below `/v1/projects/{project_id}` unless it already
shows that prefix. Profile/template apply produces immutable revisions; running
epics retain pinned revisions until their explicit preview/apply upgrade.

Advisor/Committee callers are not authorized merely because they hold the
Operator bearer. The application service also proves the calling run's exact
SeatBinding and the pinned profile's allowed action. Consultative seats remain
read-only even though their settlement is a Kontor write.

## Native capacity and admission

### Ownership

`kontor-accounts` owns raw provider/account observations, derived availability,
operator override, reserve/cooldown/admission/recovery policy and the adaptive
state transition. `kontor-scheduler` continues to own capacity arithmetic and
ready-batch selection. `kontor-daemon` composes current provider collectors and
runtime inspection. The store persists the account-owned records; it does not
derive their meaning.

Port the supported collectors directly. Move only their useful typed wire
parsing/fixtures. Do not create a generic connector framework. Delete the fleet
calls from `kontor-integrations-asma` and remove every production dependency on
`AsmaExecutable` for preflight/status/block. Kontor must pass with `asma` absent
and must never read or write ASMA fleet stores, event files or AgentsRoom
descriptions.

Persist a raw observation first, keyed by the collector's stable observation
identity, then derive availability/pressure and update adaptive state in the
same service transaction. Raw evidence and operator override remain separate;
an override never rewrites what the provider reported.

### Capacity routes

| Tool | Method and path | Tier | Contract |
| --- | --- | --- | --- |
| `kontor_capacity_config_get` | `GET /v1/capacity/configuration` | Admin | Current immutable configuration revision and effective values. |
| `kontor_capacity_config_preview` | `POST /v1/capacity/configuration:preview` | Admin | Validates a full replacement and reports clamping/effect on current windows; no write. |
| `kontor_capacity_config_apply` | `POST /v1/capacity/configuration:apply` | Admin | Applies the exact preview under expected revision and returns receipt/readback. |
| `kontor_capacity_get` | `GET /v1/projects/{project_id}/capacity` | Observer | Raw-evidence refs, derived availability/overrides, active TeamRun total, ceiling, current adaptive width/streak/last observation and last refusal. |
| `kontor_capacity_refresh` | `POST /v1/projects/{project_id}/capacity:refresh` | Operator | Runs configured native collectors; request may select configured account ids only. Returns one receipt and the derived projection. |
| `kontor_capacity_observation_get` | `GET /v1/projects/{project_id}/capacity/observations/{observation_id}` | Observer | One redacted raw observation and its derived outcome. |
| `kontor_capacity_override` | `POST /v1/projects/{project_id}/provider-account-profiles/{account_profile_id}/availability:override` | Operator | Expected-revision override with reason/expiry; no threshold, pid or argv. |
| `kontor_seat_attention` | `POST /v1/projects/{project_id}/seat-bindings/{seat_binding_id}/attention` | Operator | Observes the exact bound seat and records typed attention evidence. |
| `kontor_seat_retire` | `POST /v1/projects/{project_id}/seat-bindings/{seat_binding_id}:retire` | Operator | Retires/releases the exact binding after supported runtime readback; never scans by name or `cwd`. |

No capacity request accepts caller-authored policy thresholds, fallback command,
process identity, provider response, cooldown timestamp or raw availability.
Those are configuration or trusted connector facts.

### Operational policy

The default remains one `CapacityConfig`, changed in place:

```text
global=16, project=8, mission=7,
account=4, provider=4, runtime=8,
adaptive={ initial=4, floor=1, ceiling=7, growth_step=1 }
```

Seed `AdaptiveAdmissionState` when an epic is applied/pinned, not lazily on the
first plan. Every scheduling snapshot restores its persisted width. Fold a new
capacity observation as follows:

```text
same observation id       -> unchanged (including streak)
pressure                  -> width=floor, streak=0
first distinct clean      -> width unchanged, streak=1
second distinct clean     -> width=min(width+growth_step, ceiling), streak=0
```

Reuse `AdaptiveWindow::restore` and call its existing `observe(Clean)` only on
the second clean observation. Do not add a second window type or reset state in
`Services::snapshot`.

The adaptive width limits new admissions in one scheduling pass. The mission
ceiling independently counts active admitted non-terminal `TeamRun` envelopes
and caps the total at seven. Count each TeamRun once; do not count its seats,
and especially do not count persistent idle SeatBindings. Pressure changes
future admission only and never cancels an admitted run.

## Registry and generated-contract rule

For each operation, land all of these in one checkpoint:

1. closed request/response DTO and `ApplicationOperations` method;
2. handler authorization and router entry;
3. OpenAPI registration and generated `contract/openapi.json`;
4. one `ToolSpec` with the same method/path/tier and closed top-level arguments;
5. generated TypeScript client schema and CLI/MCP parity tests.

Add validated registry argument kinds for topology spec, role catalog,
topology-node and SeatBinding ids rather than treating them as free text.
Nested documents stay `ArgType::Json` and are validated once by the daemon DTO
and domain.

`NON_AGENT_ROUTES` may contain only health, OpenAPI and genuine process probes,
each with a reason. Remove `/v1/commands/{kind}` from the public router/OpenAPI;
the dynamic intent route is neither a probe nor a closed semantic tool and
would bypass the registry. Existing concrete application routes supersede it.

## Coherent builder checkpoints

1. Shared DTOs, authority/idempotency/preview rules, route table, OpenAPI,
   registry and generated artifacts. Contract-only successor routes fail closed.
2. Topology specification/read/upgrade, role catalog/code help and semantic
   topology operations, all reusing the OP-01 store and OP-02 materializer.
3. Account-owned native capacity records/connectors plus configuration,
   refresh/status/override/evidence and exact-seat operations; remove the fleet
   `AsmaExecutable` edge.
4. Persisted adaptive controller and active-TeamRun accounting, followed by the
   full authorization, replay/restart, registry/OpenAPI drift and `asma`-absent
   suites.

Each checkpoint must build and its write paths must have one black-box test that
would fail on a stale revision or replayed key. Stage only files owned by this
ticket; the unrelated generated `docs/evidence/KON-MVP-18/run-*` directories in
this worktree are not OP-03 evidence and must not be included.

## Required negative proofs

At minimum, kill these shortcuts:

- an Operational route absent from both `REGISTRY` and the narrow probe list;
- observer mutation or wrong minimum tier;
- publication/apply under a stale revision, or replay creating a second effect;
- raw `role`, unknown role code or caller-supplied standard title;
- model-authored kind, parent, native id/name/`cwd`, threshold, pid or argv;
- a published or epic-pinned specification changing in place;
- topology materialization outside OP-02's exact-id/capability path;
- a capacity refresh that stores only derived state or lets an override rewrite
  raw evidence;
- any production `AsmaExecutable`, ASMA fleet store/event read or AgentsRoom
  seat description;
- a scheduling snapshot resetting the adaptive window to four;
- one clean observation growing the window, or replay growing it again;
- counting seats instead of active TeamRuns, admitting an eighth run, or
  cancelling active work under pressure;
- `epic-get` returning a null/different Team revision immediately or after
  restart;
- a contract-only successor operation reporting success before its owning
  application service exists.
