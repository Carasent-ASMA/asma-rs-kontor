# KON-OP-05 architecture handoff

Date: 2026-08-17  
Status: approved for implementation  
Scope: versioned Advisor profiles, configurable Committee templates and their
epic-local read-only runs

## Decision

Compose OP-05 behind the `ApplicationOperations` boundary fixed by OP-03. Do
not add another HTTP facade, consultation scheduler, generic workflow language
or runtime placement path.

```text
authenticated OP-03 /v1 operation
                |
                v
      kontor-daemon Services
      |       |          |
      v       v          v
kontor-teams policy   kontor-context
run rules     rules   frozen provenance
      \       |          /
       \      v         /
        OP-02 semantic topology materializer
          + exact native-id readback
```

`kontor-teams` owns the versioned Advisor/Committee definitions and their run
state machines. `kontor-policy` owns caller eligibility, the consultative
read-only capability boundary, provider diversity, finding order, conjunctive
aggregation and the round budget. `kontor-context` resolves one immutable
context snapshot with provenance. `kontor-store` persists definitions, runs,
findings, dispositions, pins, bindings and receipts. `kontor-daemon::Services`
resolves authenticated authority and DTOs, executes one resumable command and
supplies semantic placement effects through the OP-02 topology/container and
SeatBinding path.

The daemon must not make consultation state authoritative in a process-local
map. The first invoke freezes every Kontor id, definition reference, context
hash, topology-node id and role-slot identity before the first runtime effect.
A restart or lost response reconciles those ids and the missing suffix; it does
not create a second ASW, CSW, seat, finding or verdict.

## Verified baseline

OP-01 supplies the data vocabulary OP-05 consumes:

- immutable topology specifications and role-catalog revisions;
- `ASW` and `CSW` as data-defined, read-only ESW child/session-host kinds;
- `TSC` as a read/import alias for `CSW`, never a new-state kind;
- `ADVISOR`, `COMMITTEE_MEMBER` and `JUDGE` as controlled consultation seat
  types, not free-form delivery roles;
- the built-in topology's `advisor_kind` and `committee_kind` selectors.

OP-02 supplies the only accepted production placement path:

- topology-node identity and exact native binding/readback;
- capability-dispatched `native_child` and `session_host` projection;
- ESW-as-native-project and child workspace placement;
- durable SeatBinding and liveness evidence;
- `placement_blocked` before launch when project, workspace, kind, `cwd`,
  identity or readback disagrees.

OP-03 supplies the public contract and composition boundary:

- eleven Advisor/Committee `/v1` operations, handlers, OpenAPI operations,
  registry entries and CLI/MCP parity;
- immutable profile envelopes, preview/apply, expected revision, idempotency
  and receipt conventions;
- `AdvisorConsultation` and `CommitteeConsultation` semantic topology targets;
- Operator plus server-side SeatBinding authority for run operations;
- typed `Unavailable` stubs in `Services` for the successor behavior.

OP-04 supplies the concrete epic base:

- one frozen MiniProject/ESW with one exact native epic project;
- one ECP containing distinct persistent LSA and TPM SeatBindings;
- frozen project topology, role-catalog, roster and configuration references;
- durable semantic-effect and lost-ack patterns for child workspaces and seats.

The current `Services::resolve_scope` intentionally refuses the two
consultation targets because no durable run yet resolves their epic. OP-05
replaces only that licensed refusal and the eleven family stubs. It reuses
`ensure_container`, exact native readback and the existing topology projection;
it must not recursively call a public topology operation or accept native ids
from a consultation caller.

## Service composition

### Domain and repository boundary

Persist these facts through the existing repository/store transaction style:

1. immutable Advisor-profile and Committee-template revisions plus each
   project's current catalog revision;
2. one AdvisorRun or CommitteeRun identity, semantic scope, bounded question,
   requester SeatBinding and frozen definition reference;
3. the canonical resolved context document, its hash, provenance/redaction
   records and every source cursor/reference used to build it;
4. stable ASW/CSW topology-node ids, declared role slots, SeatBindings and exact
   native project/workspace/session readback;
5. immutable Advisor advice artifacts and append-only advice dispositions;
6. immutable Committee member findings, evidence references, dissent, Judge
   aggregate and round lineage;
7. current run revision, typed attention state and command receipts.

Use the existing command-receipt/idempotency mechanism at the daemon boundary.
The same key plus the same canonical intent returns the original projection;
the same key plus changed intent is `idempotency_conflict`. Runtime display
names, `cwd` and observed ids are evidence, never idempotency identity.

Profile/template previews are pure reads. They deserialize into the family
schema, validate and canonicalize it, then return stable violations and a hash;
they commit no draft, receipt, id or aggregate. Apply repeats validation against
the current project revision, compares the hash and publishes only version one
or the exact next immutable version.

### Context provenance

An invoke body names a semantic scope, one exact published revision and a
bounded question. It may name typed references to already-authoritative Kontor
evidence; it may not upload arbitrary files, prompts, memory or runtime state.
The server resolves and freezes:

- realm, project, epic and optional ticket scope;
- requester account, AgentRun, SeatBinding, role code and allowed action;
- question bytes/hash and the normalized title topic;
- profile/template id, version and canonical definition hash;
- epic-pinned topology, role-catalog, Advisory and applicable policy revisions;
- profile-declared skills, approved files/memory and provider/model/context
  policy;
- resolved evidence ids, source revisions/cursors, content hashes, redactions
  and omissions;
- budget, timeout, consultation count and round lineage.

Represent the resolved input with the existing `kontor-context`
`ResolvedContextPack`/canonical-document machinery. Store the provenance next
to the run and deliver only that frozen document to seats. A later file, memory,
profile or epic change cannot alter an already invoked consultation. Missing a
required source or failing redaction blocks before launch; the caller cannot
replace it with prose.

### Semantic effect adapter

The daemon-side OP-05 effect adapter has four bounded jobs:

- resolve `AdvisorConsultation` or `CommitteeConsultation` from the durable run
  to its exact epic and data-defined ASW/CSW kind;
- ensure/materialize one child workspace inside the exact bound ESW native
  project through OP-02, then verify project, workspace, kind and `cwd`;
- create/reconcile only the frozen profile/template SeatBindings and their
  native sessions in that workspace;
- deliver the frozen context, ingest bounded output evidence and reconcile
  lifecycle/receipt state.

There is no name-based adoption after identity has been persisted, no fallback
project inference and no generic `create_node(kind, parent)` escape hatch.

## Successor contracts that gain behavior

Every path below is under `/v1/projects/{project_id}` and keeps the authority
tier, handler, OpenAPI operation and `ToolSpec` fixed by OP-03.

| Contract | Behavior supplied by OP-05 |
| --- | --- |
| `GET /advisor-profiles` | Return the project's published immutable Advisor revisions in deterministic order with aggregate revision and snapshot cursor. |
| `POST /advisor-profiles:preview` | Parse and validate a complete typed Advisor definition, allowed caller/scope rules, context sources, runtime policy and budgets; return violations plus `preview_hash`; write nothing. |
| `POST /advisor-profiles:apply` | Revalidate the preview and expected project revision, publish the next immutable profile version and return one receipt. It creates no ASW or seat. |
| `POST /epics/{epic_id}/advisor-runs:invoke` | Prove the requester SeatBinding and allowed scope, freeze context/profile/budget/ids, ensure one ASW and one Advisor seat, launch or reconcile it, and return the durable run. |
| `POST /advisor-runs/{advisor_run_id}/settle` | Record the Advisor's immutable output through its bounded evidence authority, then record the requester's or owning LSA's accepted/partially-accepted/rejected/superseded disposition and rationale. Operational side effects require separate typed commands. |
| `GET /committee-templates` | Return published immutable Committee revisions in deterministic order with aggregate revision and snapshot cursor. |
| `POST /committee-templates:preview` | Parse and validate slots, context visibility, provider diversity, quorum/aggregation, verdict schema, budgets and round limit before effects; return violations plus `preview_hash`; write nothing. |
| `POST /committee-templates:apply` | Revalidate the preview and expected project revision, publish the next immutable template version and return one receipt. It creates no CSW or seat. |
| `POST /epics/{epic_id}/committee-runs:invoke` | Prove caller authority, freeze template/context/slot/round ids, reject diversity or placement violations, ensure one CSW and every declared seat, then launch/reconcile the run. |
| `POST /committee-runs/{committee_run_id}/findings:record` | Resolve the exact caller SeatBinding server-side and append that slot's typed finding or Judge aggregate once for the current round. Conflicting replacement is refused. |
| `POST /committee-runs/{committee_run_id}/settle` | Recompute the template rule from persisted findings, refuse premature or contradictory Judge output, freeze the typed outcome/dissent/evidence and return the original result on replay. |

The existing generic topology inspect/drift/ensure/materialize/retire/archive
operations remain the diagnostic and explicit lifecycle surface. OP-05 does not
add a generic consultation-node route.

### OP-03 DTO corrections required before enabling the routes

Keep the routes and shared profile revision envelope, but close the bodies that
currently cannot represent the promised behavior:

- deserialize `ProfilePreviewRequest.definition` and
  `ProfileApplyRequest.definition` into an `AdvisorProfileSpec` or
  `CommitteeTemplateSpec` selected by the route; reject unknown fields and hash
  the canonical typed value rather than storing arbitrary JSON;
- extend `InvokeConsultationRequest` with a closed semantic scope (`epic` or a
  ticket id belonging to that epic), typed existing-evidence references and an
  optional predecessor run for the one permitted re-review; native placement
  remains absent;
- replace raw `RecordFindingsRequest.findings` with a closed tagged member
  finding/Judge aggregate carrying the round, typed outcome, bounded rationale
  and existing evidence references; the server derives the slot and caller;
- replace the shared bodyless settlement request with family-specific actions:
  Advisor output/disposition and Committee settlement. Preserve expected
  revision and idempotency in every write;
- project typed lifecycle, disposition, round, verdict, evidence completeness,
  attention and topology/SeatBinding references instead of opaque `String`
  state and a finding count alone.

Regenerate OpenAPI/client/registry parity artifacts in the same implementation
change. These are narrow corrections to OP-03's successor bodies; they do not
license new routes or a second wire vocabulary. The stubs remain `Unavailable`
until the corrected bodies and durable services are composed end to end.

## Advisor behavior

An Advisor profile is an immutable, versioned policy document. It declares its
stable id/version/name, short display name, domain/expertise, bounded behavioral
prompt, provider/model/context policy, approved skills/files/memory, allowed
caller roles/scopes, input and output requirements, budget, timeout and
consultation limit. It never carries a mutation, scheduler, topology, gate
waiver or destructive capability.

Invocation performs all authority, context and placement checks before the
first native effect. One successful invocation owns one stable AdvisorRun id,
ASW topology-node id, ASW native workspace id and Advisor SeatBinding id. The
workspace is named:

```text
ASW · <scope key> · <topic>
  Advisor · <profile short name>
```

It is a sibling of ECP, TSW and CSW inside the exact epic project, never inside
a TSW/CSW and never a standalone project. The current Operational analysis's
`ASW · ...` name is authoritative for new state; the older architecture text's
`Advice · ...` spelling is a display-only historical form and is not emitted.

The Advisor may read only its frozen context and submit its own bounded advice
artifact. This evidence-only submission authority does not permit it to invoke
other writes. The requester or owning LSA then records one append-only
disposition:

- `accepted` — the advice was adopted;
- `partially_accepted` — named parts were adopted;
- `rejected` — it was considered and not adopted;
- `superseded` — a later recorded decision replaces an earlier disposition.

Each disposition stores bounded rationale and references to any separately
authorized typed command receipts. It never rewrites the advice, grants
authority, waives a gate or claims that a command ran. An inconclusive response
records the artifact and enters `NEEDS_HUMAN` only with a recommended resolution
and the tried deliberation path; it cannot silently pass or remain indefinitely
running.

## Committee behavior

### Versioned template

A Committee template declares ordered stable role-slot ids, per-slot role,
specialty, behavior, skills/files/context visibility and provider/model policy;
independence and diversity constraints; aggregation protocol and typed verdict
schema; optional Judge or deterministic aggregator; quorum/threshold or
conjunctive rule; dissent behavior; budget; and a bounded round limit.

Cardinality is template data. The builder must exercise the same Admin
preview/apply and run path with test-only two- and five-seat definitions. Those
fixtures are not production presets and no service may branch on seat count
three.

### `independent_review@1`

The only production preset in OP-05 is `independent_review@1`:

1. exactly two reviewer slots and one Judge slot are frozen at invoke;
2. the two reviewers resolve to contrasting provider families before any
   topology or runtime effect; labels or different models on one provider do
   not satisfy diversity;
3. both reviewers receive the same frozen question/shared evidence but cannot
   read one another's first finding;
4. each reviewer records one immutable first finding for the round;
5. only after both findings are durable may the Judge read them and submit an
   aggregate;
6. the service recomputes the outcome; the Judge explains and records it but
   cannot override the rule;
7. dissent and every evidence reference remain visible in the settled round.

The verdict is conjunctive:

```text
COMPLIANT     iff both required reviewers say COMPLIANT
                 and both required evidence sets are complete
NON_COMPLIANT otherwise, once both required findings are durable
```

A missing finding keeps the run awaiting findings and blocks Judge submission.
A recorded finding with missing required evidence counts against the gate and
therefore settles `NON_COMPLIANT`; it is never omitted from the denominator.
Malformed or genuinely inconclusive deliberation that cannot produce the typed
aggregate enters `NEEDS_HUMAN` with a recommended resolution and complete tried
path rather than guessing either verdict.

### Immutable rounds and bounded remediation

Round one is the initial decision. A `NON_COMPLIANT` result may be followed by
at most one authorized remediation/re-review round. Round two is causally linked
to round one, receives the prior verdict/evidence plus the authorized
remediation evidence, and reuses the same compatible CSW and SeatBindings after
identity/readback and compaction checks. Incompatible or unusable seats follow
the existing explicit retire/archive/replacement policy; they are never
silently duplicated.

Every finding and aggregate is keyed by run lineage, round and frozen role-slot
id and is immutable. Exact duplicate submission replays; a different value for
the same key conflicts. Round two appends; it cannot edit round one. A third
round is `remediation_budget_exhausted` and becomes `NEEDS_HUMAN` with the two
rounds in its tried path.

OP-05 enforces this ceiling and exposes the immutable evidence. OP-06 owns the
Completion Profile decision to authorize remediation work and route another
round; OP-05 does not reopen tasks, create remediation tasks or advance
completion on its own.

## Topology and phase implications

- ASW and CSW are logical children of the epic ESW and native children inside
  that ESW's already-bound project. Neither is a native project.
- An ASW owns exactly one Advisor SeatBinding. A CSW owns the frozen template's
  declared SeatBindings; ticket- versus epic-scoped purpose changes its scope
  key, never its node kind.
- New state emits `CSW` only. Historical `TSC` imports normalize to the same
  logical CSW identity before deduplication and never create a sibling node.
- `read_only=true` in the pinned topology specification is necessary but not
  sufficient: the resolved work/runtime capability and server-side operation
  allowlist must also deny source mutation, Jira, scheduling, gate waiver,
  topology mutation and raw Paseo tools.
- Consultation runs are not Tasks or TeamRuns. They do not change task phase,
  start delivery, consume an epic mission slot or count an idle persistent seat
  as active work. Provider/account capacity and consultation budgets still
  govern their launches.
- Advice, findings and a Committee verdict are evidence. They affect a gate or
  phase only when an authorized owning workflow consumes that evidence through
  its existing typed transition.
- Invocation, output capture, disposition, findings and settlement each use
  expected revision and receipts. A chat message or native session completion
  alone advances nothing.
- `NEEDS_HUMAN` is an explicit attention state with recommendation and tried
  path, not a synthetic success, stalled `running` state or gate waiver.

## Composition checkpoints

1. Add typed Advisor/Committee specifications, validation, immutable
   preview/apply/read persistence and the narrow OP-03 DTO corrections; seed
   only `independent_review@1`.
2. Compose frozen context provenance and repository-backed Advisor invocation,
   one-ASW/one-seat placement, immutable output and disposition; prove exact
   ESW refusal, read-only authority and lost-ack replay.
3. Compose template-driven Committee invocation, variable cardinality,
   provider diversity, independent immutable findings, Judge ordering and
   server-recomputed conjunctive settlement.
4. Compose the one-remediation-round lineage, compatible-seat reuse,
   `NEEDS_HUMAN`, TSC import normalization and restart reconciliation; then
   replace all eleven OP-03 `Unavailable` stubs with projections/receipts from
   the same durable service.

Each checkpoint must build. Do not enable one route with an in-memory aggregate,
unvalidated JSON or fake success while its durable composition is incomplete.

## Required proofs

- unknown/non-current definitions, stale revisions, malformed specs and
  unregistered actions refuse before effects;
- unauthorized callers and a valid Operator bearer without the required exact
  SeatBinding/allowed action refuse before effects;
- caller-authored native ids, names, parents, kinds, files, prompts and raw
  topology payloads are absent or rejected;
- ASW and CSW materialize once inside the exact epic project across duplicate,
  lost-ack and restart retries; no ticket worktree or fallback project appears;
- Advisor context bytes/hash/provenance remain stable after source or profile
  changes, and disposition never mutates advice or performs its referenced
  command;
- consultative credentials can read frozen context and submit only their own
  bounded evidence; every operational mutation and raw Paseo action is denied;
- `independent_review@1` matches its full truth table, blocks same-provider
  reviewers and prevents Judge submission before both first findings;
- missing required evidence yields `NON_COMPLIANT`, dissent remains durable and
  a Judge cannot turn a failing conjunction into `COMPLIANT`;
- test-only two- and five-seat templates use the same validated Admin/run path
  and kill any hard-coded three-seat implementation;
- findings, aggregates and prior rounds reject conflicting replacement; the
  one permitted re-review reuses compatible identities and a third round
  refuses as exhausted;
- historical TSC input reads as the existing CSW and new state never emits TSC;
- inconclusive Advisor/Committee paths reach `NEEDS_HUMAN` only with a
  recommended resolution and the roles, consultations and rounds already
  tried;
- no OP-05 path creates or advances a Task, TeamRun, completion run, Jira item
  or delivery phase.

## Out of scope

OP-05 does not implement Completion compilation, remediation-task creation or
closeout (OP-06), Jira create/link or authority cutover (OP-07), final
cross-feature CLI/MCP assembly (OP-08) or diagnostic UI (OP-09). Jury,
Conjunctive Compliance and Deliberative Panel remain explicit protocol
deferrals; interactive debate is also deferred. Their absence is not permission
to hide them behind `independent_review@1`, seed speculative presets or add a
protocol engine beyond the versioned template path described here.
