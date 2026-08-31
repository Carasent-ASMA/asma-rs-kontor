# ASMA Kontor Architecture

> **Status:** Current pre-1.0 repository contract. Synchronized with the tree on
> 2026-08-26.
>
> **Scope:** Governing principles, authority boundaries, consistency model,
> technology choices and extension rules for `asma-rs-kontor`.

## Purpose

Kontor is the durable project control plane above replaceable agent execution
runtimes. It supervises work that may outlive one process, one model, one
provider account or one orchestration product.

It exists to serve two values, and every rule below is derived from one of them.

**Autonomy** — the fleet runs the work without an operator driving each step,
and runs it the way *this* operator would. That needs two halves:

- **Deterministic rails.** The workflow — phases, gates, roles, teams, topology,
  completion, consultation — is versioned data the kernel executes, not
  behaviour a model improvises per session. Same inputs, same admission
  decision, same refusal, on every restart and from every client. Rails are what
  keep a fleet from inventing its own process: an agent cannot talk its way past
  a rule in chat, because chat advances nothing.
- **Captured personality.** What the operator decided, prefers and keeps
  correcting becomes durable, approved, versioned memory, injected into every
  seat's Context Pack deterministically — not re-explained each session and not
  scraped from a transcript.

Autonomy is never "the model decides". It is "the operator's decisions, made
once, become the system's behaviour, always".

**Delivery Quality** — the unit of value is a *completed* epic or ticket, and
completion is defined, checked and independently verified rather than asserted.
Delivered means production-ready, tested, reviewed by an authority that did not
do the work, with regression risk actively reduced and evidence that outlives
the runtime and the model. The sharpest consequence: **a fully green task graph
does not complete an epic.** Only a compliant independent verdict plus complete
closeout evidence does.

Beyond those, it exists because execution and control have different lifecycles:

- runtimes are best at native sessions, tools, permissions, transcripts and
  provider authentication;
- a project control plane must persist dependencies, policy, intent, evidence,
  freshness and reconciliation across runtime restarts and replacements.

Putting both responsibilities into every runtime would duplicate authority and
make cross-runtime recovery impossible to reason about.

## System context

```text
Humans and agents
  CLI | MCP | React console | Tauri desktop
                  |
       authenticated /v1 loopback API + SSE
                  |
            kontor-daemon (one Realm)
      +-----------+------------+-------------+
      |           |            |             |
   domain      SQLite       scheduler      policy
   profiles    events       + leases       + evidence
      |           |            |             |
      +----------- runtime adapter contract--+
                       |
              Paseo  (ao | codex refused)
                       |
                native sessions

Native connectors inside the daemon: Jira | provider usage
```

The daemon is the composition root. API, CLI, MCP and UI are clients of the
same realm-qualified contract; none creates a parallel scheduler or state
store.

## Authority boundaries

| Fact or effect | Authority |
| --- | --- |
| Realm, projects, tasks, dependencies, profiles and gates | Kontor |
| Scheduler decisions, leases, policy verdicts and receipts | Kontor |
| Desired runtime command and last observed native evidence | Kontor records |
| Native process, session, transcript, tools and provider authentication | Selected runtime |
| Provider capacity, cooldown and quota headroom | Kontor's native provider connector |
| External ticket workflow | Jira, through Kontor's native `kontor-jira` connector |
| A project's backlog or memory before its cutover attestation | AgentsRoom, per `(project, subject)` |

The core rule is **one writer per fact**. Kontor uses supported adapter or API
interfaces and never edits Paseo, AO, Jira or AgentsRoom internal stores. No
Kontor crate or process invokes the `asma` CLI.

Write authority for a project's `memory` and its `backlog` is a fact about
`(project_id, subject)`, not a realm-wide flag. A project created in Kontor is
native and writable from its first instant; a project whose facts came from
AgentsRoom stays read-only for that subject until its import and read-back
attestation succeed. Both states can be true of one realm at once.

## Consistency model

### Desired, observed and derived state

Kontor keeps three facts instead of one optimistic status:

1. **Desired** — what a durable command requested.
2. **Observed** — what the runtime actually confirmed, with native identity,
   generation, cursor and observation time.
3. **Derived** — the safe operator-facing interpretation.

`stale`, `diverged`, `runtime_unavailable`, `orphaned` and `lost_contact` are
non-terminal. Timeouts and closed streams do not become success or failure.

### Intent before effects

Runtime-changing commands use an outbox/receipt protocol:

```text
persist intent + desired state + event
  -> dispatch with stable correlation/idempotency
    -> record acknowledgement
      -> inspect authoritative native state
        -> confirm or expose uncertainty
```

A restart classifies unsettled receipts from durable evidence. It never
blindly repeats an effect whose first dispatch may have succeeded.

### Runtime-owned session content

Kontor stores bindings and continuity evidence, not a second transcript.
History is paged from the runtime, live subscription starts strictly after its
anchor, and an epoch/sequence gap requires refetch. CLI, MCP and UI receive
session content through the daemon so runtime credentials never reach clients.

### Persistent seats and workspaces

A team template declares stable role-slot IDs. One non-terminal native session
may occupy a `(team_run_id, role_slot_id)` at a time. Admission is claimed
atomically before the first native effect; replay and concurrent launch are
refused. A replacement must cite and close the prior binding.

For Paseo, one Jira epic maps to one project. Inside it: one **ECP** (Epic
Control Plane) workspace holding the epic's persistent `LSA` and `TPM` seats;
one **TSW** (Ticket Session Workspace) per ticket holding that ticket's
persistent role seats; and sibling read-only workspaces for consultations —
`Advice · …` for one Advisor, **CSW** (Committee Session Workspace) for a
Committee. The Git worktree is separate checkout/isolation evidence, not the
workspace identity. `PASE` and `TSC` are historical spellings of TSW and CSW and
survive only as read/import aliases.

Backlog identity is deliberately split into three layers. Jira owns the full
confirmed tracker key (`ASMA-8001`). Kontor owns one immutable, case-insensitively
unique epic namespace per project (`KOP`), allocated from title characters or
selected manually. A native item code (`KOP-8001`) is a display-only projection
of that namespace and the confirmed Jira numeric suffix; it is never persisted
as a second Jira binding and is never used to reconstruct one. Schema v72
preserves legacy codes as immutable evidence, quarantines duplicate or invalid
values, and allows a separate active assignment beside quarantined evidence.
Pre-v72 epics with no legacy code remain readable and operable; assignment is an
explicit write, and any topology needing the projection stays blocked until it
has happened.

Operational topology revision 1 remains byte-immutable. Revision 4 opts into the
typed `ITEM_CODE` token and centered-dot separator, yielding names such as
`ESW · KOP-8001`, `ECP · KOP-8001` and `TSW · KOP-7869`. Projects and epics move
to it only through their existing preview/apply selection and upgrade seams. A
template asking for `ITEM_CODE` fails `placement_blocked` before any runtime
mutation unless the epic namespace is active and the relevant Jira binding has
exactly one confirmed readback.

Jira materialization recovery preserves the failed create batch as the sole
intent. Schema v74 adds an append-only recovery ledger keyed to its exact item,
ordinal and marker; the daemon may adopt an already-created Jira issue only when
project, issue kind, parent, summary, description and marker all match that
original create plan. Response items are mapped by ordinal, never by connector
array position, and a confirmed epic binding cannot be changed to another Jira
key.

Runtime permissions raised by a Committee filler remain runtime-owned, but the
authority to answer them is a Kontor control-plane effect. Inspection addresses
the exact Committee run, logical SeatBinding and attested native filler. Schema
v75 records the exact occupancy generation, native id, request id, UUIDv7
response identity and decision before dispatch; only runtime acknowledgement
advances it to confirmed. Confirmed calls replay without a second native effect,
while confirmation-unknown dispatches fail closed. The `leadership` MCP profile
contains only completion read/remediation plus these exact Committee permission
operations.

## Deterministic scheduling and policy

A model may recommend work but cannot write scheduler truth. Admission is a
deterministic reduction over dependencies, serialization/module collisions,
verified worktree leases, runtime freshness/capabilities, provider/account
health and quota headroom, budgets, permissions, and any active narrowing.

Admission is **default-allow**. Ready work does not have to name an
authorization: a grant only *narrows* — a start window, a concurrency bound, a
selected task set — and a disarm is an explicit stop rather than a return to an
unarmed default. Bounded autonomy that has to be granted per task is not
autonomy; unbounded autonomy that cannot be narrowed or stopped is not safe.
Arming to "enable" work is therefore a mistake: it can only make the admissible
set smaller.

Quota headroom has one deliberate status caveat. The account-before-rung
resolver computes typed `Admit`, `Wait` and `NeedsHuman` outcomes, and launch
honours `Admit`. The delivery launch boundary still has to return a model rung,
however, so it currently drops the reset/escalation payload from `Wait` and
`NeedsHuman` and preserves the adapter's typed provider-outage refusal path.
Automatic pre-launch parking until the computed reset is not shipped; mid-run
quota detection and successor handoff are separately open.

The calendar dimension is implemented in `kontor-calendar` and is reached by no
route and no tool, so every project currently resolves to `unrestricted`. That
is stated rather than implied, because a tested crate nothing calls is a claim
the product does not honour.

Every refusal is explainable. Lowering an adaptive admission window stops new
work but does not cancel already bounded work. Gate authority and recovery
budgets come from immutable work-profile/team snapshots, not role-name guesses.

## Completion is a machine, not a claim

Guardrails stop unsafe work; they do not decide whether an epic is finished.
That is a separate configurable machine and it is the core of Delivery Quality.

**Advisors and Committees** are read-only consultations built from versioned
specifications. An advisor produces one expert opinion in one seat; a committee
produces a multi-member deliberation with a declared protocol, aggregation rule
and verdict schema. Both freeze every semantic input — pinned policy, context,
model rungs, scope — before any runtime effect, so a later configuration change
cannot retroactively alter what a finding was based on. Neither can grant missing
authority, approve destructive work, waive a gate or reset a rejection counter.
Their output is evidence; the caller records whether it was accepted, partially
accepted, rejected or superseded.

The shipped preset is `independent_review@1`: two reviewers on deliberately
contrasting providers plus a judge. Each reviewer records one finding, cannot see
the other's, and must not wait for it. The judge explains the outcome the
conjunctive rule already produced, including dissent — it cannot turn a failing
conjunction into a passing one. A committee seat runs under the `consultation`
serve profile, which reaches its own consultation aggregate and nothing else.

**Epic completion** compiles a pinned profile into ordinary task, team, gate and
receipt nodes — there is no second scheduler and no workflow language:

```text
Tickets -> Integration -> Verdict(n) -> [Remediation(n) -> Verdict(n+1)]* -> Closeout -> Done
                              |                    |
                              +--------------------+--> NeedsHuman
```

Closeout requires all six prerequisites — merge, release, version inventory,
summary, notification, archive. `advance` is a pure, revision-checked state
machine over observed evidence: no runtime, no clock, no filesystem, no external
command. `NeedsHuman` is a real terminal reached by exhausted rounds, missing
authority, unresolved disagreement or incomplete evidence — not a status an agent
sets to make progress.

Three rules learned in the field and now binding:

1. **No waiver at completion.** A non-compliant aggregate verdict opens
   remediation; a committee cannot waive it for itself.
2. **Re-freeze after drift.** Product-code change after the evidence freeze
   invalidates the part of the bundle it touches, including recorded mutation
   anchors.
3. **Remediation creates real work.** A finding becomes a tracked ticket with an
   owner, and remediation authority is bound to a seat occupancy generation so a
   replaced native filler cannot replay its predecessor's proposal.

## Technology decisions

| Choice | Why it was selected | Deliberately deferred/rejected |
| --- | --- | --- |
| Rust 2024 | Explicit state/concurrency boundaries, strong domain types and a distributable local control plane | A second Node service runtime for core authority |
| Tokio + Axum | Mature async subprocess, HTTP and SSE composition in the Rust process | Multiple service processes for the MVP |
| SQLite (`rusqlite`, WAL) | Embedded durability, transactions, crash recovery and simple realm-local backup | Network database and sync semantics before they are needed |
| Ordered SQL migrations | Inspectable storage evolution with deterministic startup | Runtime-generated schema drift |
| UUIDv7 + realm-qualified envelopes | Opaque sortable IDs without confusing native runtime IDs for Kontor IDs | Provider IDs as primary keys |
| JSON + Serde + checked-in OpenAPI | One versioned wire contract shared by Rust, TypeScript and tests | Hand-maintained client types |
| Clap CLI + `rmcp` stdio server | Human and agent front doors over one operation/capability catalogue | Separate CLI and MCP business logic |
| React + TypeScript + Tauri 2 | One responsive console with native local packaging and secure credential storage | A desktop-only UI or a second embedded scheduler |
| Adapter contract | Replaceable execution planes with declared evidence grades/capabilities | Reimplementing provider sessions, terminals and auth |
| Native Jira and provider connectors | Keep each external effect under one typed writer inside the daemon, with receipt-backed observe/apply/refetch semantics | Delegating control-plane effects to `asma` subprocesses or editing external stores |
| Exact dependency pins + lockfiles | Reproducible pre-1.0 builds and auditable license/advisory state | Floating dependency resolution |
| `MIT OR Apache-2.0` | Permissive reuse plus an explicit Apache patent grant | A custom source-available license |

Storage and runtime implementations are replaceable behind contracts. A new
choice must first prove parity for transactions, migrations, receipts,
reconciliation, backup and continuity; novelty alone is not a reason to move a
control-plane boundary.

## One vocabulary: registry, MCP and CLI

The CLI and the MCP server expose the same operations at the same authorities
with the same arguments, because the CLI command tree is **generated from
`kontor_mcp::REGISTRY`**. Adding a tool adds a command; a command naming a route
the registry does not have cannot be written. The only thing the CLI decides is
spelling — `kontor_epic_apply` becomes `kontor epic-apply`, mechanically — and a
drift test asserts the two lists are the same list.

Each registry row is one tool: name, minimum caller tier, HTTP method, `/v1` path
template, operation kind and argument schema. The JSON Schema a client is shown is
derived from the same rows the dispatch path validates against, with
`additionalProperties: false` on every tool.

Authority is the credential and nothing else. A realm has three secrets; a process
holds one and therefore *is* one seat. There is no per-call escalation argument
and no per-role policy inside `kontor-mcp` — running at two authorities means
running two servers.

| Tier | Reaches |
| --- | --- |
| `observer` | Reads, including session content |
| `operator` | Reads plus the work path: claim, settle, gate verdicts, session follow-up, intake, memory proposals, consultation settlement |
| `admin` | Everything: project and account creation, epic apply, arming and disarming, selection correction, cutover attestation, gate waiver |

Two refinements matter. A gate *waiver* is an authority-changing decision rather
than an ordinary verdict, so `kontor_gate_record` demands `admin` when its verdict
is `waived` — checked in the registry before the request exists and enforced again
by the daemon. And admin is not a domain bypass: revisions, idempotency,
admission, runtime capabilities, evaluator roles, evidence and closure gates are
enforced against it exactly as against a worker's credential.

A **serve profile** narrows presentation *and* the callable set within a tier, and
can never widen it. Profiles are declared in the registry beside the tier
declarations — deliberately not in a seat file, because a free-form tool list in
configuration would be a second authority model that drifts. `worker` is the
everyday delivery surface; `consultation` is an advisor or committee filler's
minimum; `leadership` is completion read/remediation plus exact Committee-seat
permission inspection and response. An unknown profile name
refuses to start. See [`crates/kontor-mcp/seats/README.md`](crates/kontor-mcp/seats/README.md).

This is also the context-tax control: a seat is given the surface its role needs,
not the whole registry, so it spends its context on the work rather than on tool
schemas.

## Security model

- The MVP binds loopback only; wildcard/non-loopback addresses are refused.
- Every non-health route requires a realm bearer credential and a capability
  tier (`observer`, `operator` or `admin`).
- Host and Origin are validated before route handling.
- Secrets are references; DTOs, domain rows, logs, exports and process arguments
  have no field for secret values.
- Each state root has one exclusive daemon lock, database, credential file and
  immutable realm ID.
- Clients never receive runtime endpoints or provider credentials.
- Remote access, multi-user tenancy and realm federation are post-MVP security
  designs, not hidden flags.

See [Security](SECURITY.md) for reporting and supported-version policy.

## Runtime extension contract

A runtime adapter must:

1. implement the shared `RuntimeAdapter` instead of branching core behavior;
2. declare only capabilities and limits it can prove;
3. keep native IDs as correlation evidence, never Kontor aggregate IDs;
4. perform preflight/admission before native effects;
5. support fresh inspection and idempotent preparation where claimed;
6. preserve history/live cursor semantics and expose gaps honestly;
7. ship recorded offline fixtures, common contract tests and mutation evidence;
8. report unsupported operations without side effects.

Trust grades determine what Kontor may conclude. An advisory adapter can be
useful for discovery without being allowed to autonomously drive or close work.

## Repository boundaries

The workspace is split by authority rather than by technical layer alone:

- `kontor-core`, `kontor-store`: durable truth and transactions;
- `kontor-runtime*`: replaceable native execution;
- `kontor-context`, `kontor-accounts`, `kontor-teams`, `kontor-profiles`:
  immutable execution inputs;
- `kontor-scheduler`, `kontor-policy`, `kontor-calendar`: admission, safety and
  epic-completion compilation;
- `kontor-jira`, `kontor-intake`: external boundaries — a native connector and
  the durable event-intake seam;
- `kontor-api`, `kontor-daemon`, `kontor-cli`, `kontor-mcp`: one public control
  contract, one tool registry and its transports;
- `apps/*`: projections over the public API, never independent truth.

## Explicit non-goals for the MVP

- implementing an LLM, coding harness, terminal multiplexer or provider login;
- replacing Paseo, Agent Orchestrator, Codex, AgentsRoom or Jira;
- storing a duplicate native transcript/token stream;
- remote bind, multi-host workers, multi-user tenancy or realm federation;
- automatic task decomposition with model-written scheduler state;
- a hard-coded catalogue of work types or one universal agent team.

## Detailed ASMA design

The repository contract above is intentionally concise. The full implementation
baseline and active epic plan are maintained in the parent polyrepo:

- [Kontor governing principles](https://github.com/Carasent-ASMA/asma-modules/blob/master/_docs/ai-orchestration/architecture/2026-08-26-11-30-architecture-kontor-governing-principles.md) — Autonomy and Delivery Quality in full, with the fourteen principles and their honest gaps
- [Kontor control-plane architecture](https://github.com/Carasent-ASMA/asma-modules/blob/master/_docs/ai-orchestration/architecture/2026-08-08-20-12-architecture-asma-kontor-control-plane.md)
- [Kontor Operational MVP plan](https://github.com/Carasent-ASMA/asma-modules/blob/master/_docs/ai-orchestration/plans/2026-08-14-23-21-plan-kontor-operational-mvp.md)

Those documents govern ASMA-specific rollout. This file governs the public
repository boundary and must stay readable without the parent checkout.
