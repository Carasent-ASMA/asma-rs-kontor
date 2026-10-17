# ASMA Kontor Architecture

> **Status:** Current pre-1.0 repository contract. Every section was checked
> against source `0971674d` (PR #300, 2026-10-11): store schema v134, 230
> registry tools, the crate list, the serve profiles and each capability named
> below. "Merged" means present in that source. Qualification, deployment and
> realm enablement require separate receipts and are not claimed here.
> KON-OP-21 is merged through PR #170 (`080e2db3`). The
> [implementation inventory](https://github.com/Carasent-ASMA/asma-modules/blob/master/_docs/ai-orchestration/reference/2026-09-05-11-36-reference-kontor-implementation-inventory.md)
> records qualification and deployment separately and is stamped to its own
> inspected revision.
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

### Thin authority, not a small product

"Thin" describes the authority boundary. It does not mean few lines of code,
few capabilities or no user interface.

Kontor reuses mature systems for the facts they already own:

- Jira owns business workflow;
- GitHub owns repositories, pull requests and releases;
- Paseo or another selected runtime owns native sessions, transcripts, tools,
  permissions and provider authentication.

Kontor owns what none of them owns alone: cross-system orchestration,
synchronization, dependency execution, admission, policy, evidence, derived safe
state, completion and operator attention. It integrates through supported
contracts, projects native facts, and embeds or deep-links to native detail
instead of recreating a Jira editor, source host, terminal or transcript renderer.

A narrow capability may move into Kontor when retaining or adopting an entire
external product for that capability would create greater lifetime dependency,
synchronization and split-authority cost. That decision must state the strong
reason, preserve migration evidence and finish with one writer. AgentsRoom
backlog and memory are the existing example: valid bootstrap sources, followed
by attested per-project/per-subject cutover and legacy read-only state — never a
permanent half-and-half authority.

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

Native connectors inside the daemon: Jira | provider usage | GitHub App
Optional derived projection: Cognee experience recall (off by default)
```

The daemon is the composition root. API, CLI, MCP and UI are clients of the
same realm-qualified contract; none creates a parallel scheduler or state
store. One deliberate read path needs no daemon: the CLI's local fleet-policy
resolution reads the activated policy from the state root through the same
verified reader the daemon's placement uses, and writes nothing.

## Authority boundaries

| Fact or effect | Authority |
| --- | --- |
| Realm, projects, tasks, dependencies, profiles and gates | Kontor |
| Scheduler decisions, leases, policy verdicts and receipts | Kontor |
| Desired runtime command and last observed native evidence | Kontor records |
| Native process, session, transcript, tools and provider authentication | Selected runtime |
| Provider capacity, cooldown and quota headroom | Kontor's native provider connector |
| External ticket workflow and issue keys | Jira, through Kontor's native `kontor-jira` connector |
| Confirmed Jira bindings, key resolution and their evidence | Kontor |
| Repositories, pull requests and releases | GitHub |
| Publication decisions: whether a branch, commit and title match a confirmed binding | Kontor (append-only attestation ledger) |
| The `asma/publication-identity` check run and Kontor-requested squash merges | Kontor's GitHub App gateway, when installed and configured |
| The activated fleet policy and Core Team roster | Kontor state root: one activation record naming immutable, content-addressed artifacts |
| Approved experience memory | Kontor; a Cognee projection is derived and candidate-only |
| A durable desk seat declared `paseo_direct` | Its operator, in Paseo; Kontor records and reports the declaration |
| A project's backlog or memory before its cutover attestation | AgentsRoom, per `(project, subject)` |

The core rule is **one writer per fact**. Kontor uses supported adapter or API
interfaces and never edits Paseo, AO, Jira or AgentsRoom internal stores. No
Kontor crate or process invokes the `asma` CLI.

Write authority for a project's `memory` and its `backlog` is a fact about
`(project_id, subject)`, not a realm-wide flag. A project created in Kontor is
native and writable from its first instant; a project whose facts came from
AgentsRoom stays read-only for that subject until its import and read-back
attestation succeed. Both states can be true of one realm at once.

Experience memory keeps the same single writer. An experience is a typed,
versioned document proposed with resolvable evidence through the canonical
approval ledger, and only an approved revision is recallable. Recall freezes the
selected canonical bytes into the run (at most 8 experiences within 32 KiB), so a
replay reads what the run was given rather than retrieving again. The optional
`kontor-memory-cognee` projection may rank approved revisions whose policy is
`provider_eligible` (the default is `local_only`), but its answers are only
candidates: each is rehydrated from the Kontor store, provider text never becomes
prompt content or authority, and an absent, stale, failed or malformed projection
degrades to lexical recall. The transport is off by default. An ordinary daemon
start never constructs a Cognee client and refuses a `memory-cognee.json` with
`enabled: true`; only an embedding or test composition can supply one today. See
[Configuration](docs/CONFIGURATION.md) (merged in PR #291, `abe990ac`).

Publication identity is judged, not inferred. A branch follows one grammar,
`<type>/<PROJECT>-<number>-<slug>` (plus keyless `releases/v<x.y.z>`), and the
branch key must be the confirmed Jira key of the epic or task it names; a Kontor
display or backlog code is never a key. Before the ASMA workflow pushes or opens
a pull request, Kontor judges the repository, base branch, head branch, head
commit and title against that binding and records accepted and refused decisions
alike; the schema refuses any update or delete of a recorded decision (v119).
When an operator installs the GitHub App and writes `config/github-app.json`, the
daemon's gateway posts the `asma/publication-identity` check run and performs a
squash merge only after re-judging the pull request's current head, refusing a
head that moved. Without that file the gateway is inert. See
[native naming](docs/NATIVE_NAMING.md#publication-attestation) (merged in PRs
#192, #195 and #265: `2511b712`, `e4bb5fbd`, `835c5815`).

## Consistency model

### Desired, observed and derived state

Kontor keeps three facts instead of one optimistic status:

1. **Desired** — what a durable command requested.
2. **Observed** — what the runtime actually confirmed, with native identity,
   generation, cursor and observation time.
3. **Derived** — the safe operator-facing interpretation.

`stale`, `diverged`, `runtime_unavailable`, `orphaned` and `lost_contact` are
non-terminal. Timeouts and closed streams do not become success or failure.

### Operator surface contract

Kontor requires a rich mission-control surface because the joined, derived truth
does not exist in Jira, GitHub or Paseo. The operator must be able to understand,
without reconstructing the project by hand:

- the overall work picture and delivery state;
- the dependency and execution graph, including what can run next;
- what is running, waiting, blocked, failed, stale or orphaned;
- which prepared decisions need the operator now, and the consequence of no
  action;
- whether the work is merely green or independently verified and closed out.

The UI is a projection over `/v1`, never another writer or scheduler. It may
embed or deep-link native Jira, GitHub and runtime views for their detail. It
must not copy their general-purpose products. Until the joined graph, attention
inbox and delivery truth are available in one coherent view, the operator
surface is incomplete even if its individual pages work.

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

### Bounded recovery

Control-plane recovery runs the same protocol from durable evidence and is held
to four rules:

- **Uncertainty is not completion.** A lost acknowledgement, an unknown delivery
  result or an unconfirmed runtime attachment stays visible and pending until a
  fresh runtime readback proves the outcome. A delivery proof reads canonical
  history up to a frozen bound and never resends a message to establish
  delivery. An admitted run whose attachment never confirmed is replayed with its
  own admission identities and original launch key, and a launch whose answer
  was lost is recovered under the same prepared intent, so nothing is minted
  twice.
- **Exact-native recovery.** Recovery addresses one exact native identity,
  SeatBinding, occupancy generation and runtime binding. A title or an idle
  session never establishes identity, and an acknowledgement is not evidence. A
  historical transcript that omitted message identity is correlated only through
  an operator-initiated, server-owned challenge message found once after a
  boundary observed before dispatch.
- **Converge, never duplicate.** A replay or a competing recovery answers with
  the original receipt. A Core Team route succession is claimed before its first
  effect, so exactly one caller per seat and predecessor occupancy proceeds; a
  process lost mid-way completes the recorded succession instead of re-planning,
  and the receipt binds only once durable state proves the launch intent
  installed and the successor observed.
- **Supersede only what never bound.** A prepared launch intent whose launch
  never happened may have its route replaced in place — same identity, occupancy
  generation and `prepared` state — only while no occupancy is installed and no
  effect receipt names the seat. Anything else refuses without writing. An
  abandoned, never-bound replacement bridge is followed through its preserved
  ancestry and is never mistaken for the current holder.

These rules are merged across the ASMA-8110, 8111, 8119, 8187, 8190, 8196, 8202,
8203 and 8234 changes (for example PR #217 `f2d7541e`, #244 `193def19`, #264
`44663e10`, #290 `1d7a71dd`). Backup, restore, export and import are separate
operator procedures in [Recovery](RECOVERY.md); consultation-seat recovery is in
[consultation lifecycle](docs/CONSULTATION_LIFECYCLE.md).

### Runtime-owned session content

Kontor stores bindings and continuity evidence, not a second transcript.
History is paged from the runtime, live subscription starts strictly after its
anchor, and an epoch/sequence gap requires refetch. CLI, MCP and UI receive
session content through the daemon so runtime credentials never reach clients.

### Persistent seats and workspaces

A pinned Team Definition JSON revision declares stable role-slot IDs,
capabilities, hierarchy and native naming. One non-terminal native session
may occupy a `(team_run_id, role_slot_id)` at a time. Admission is claimed
atomically before the first native effect; replay and concurrent launch are
refused. A replacement must cite and close the prior binding.

For Paseo, one Jira epic maps to one project. Inside it: one **ECP** (Epic
Control Plane) workspace holding the epic's persistent `LSA` and `TPM` seats;
one **TSW** (Ticket Session Workspace) per ticket holding that ticket's
registered role seats; zero or more subject/topic-scoped **ASW** (Advisor
Session Workspace) siblings with one or more independent configured advisor-role
seats; and sibling read-only **CSW**
(Committee Session Workspace) consultations. The Git worktree is separate checkout/isolation evidence, not the
workspace identity. `PASE` and `TSC` are historical spellings of TSW and CSW and
survive only as read/import aliases.

Durable desks are the deliberate exception to that epic-scoped layout. A desk
(`DESK • ADAM`, `DESK • PR REVIEW`) is a native project that
belongs to a Kontor project, recorded as an immutable, permanent row that pins
the Team Definition revision it was ensured under. No epic owns it: epic
completion, closeout and Team Definition migration never select its nodes, and
no supported route retires or deletes it. Desks host three epic-independent
subjects:

- **Desk seats** — long-lived seats, each a dated declaration that names its
  `recovery_owner`. A `paseo_direct` seat runs outside Kontor: the daemon records
  and reports it and never censuses, adopts, launches or restarts it. A `daemon`
  seat is a persistent hosted seat that the daemon launches, and restarts only
  when an operator selects it and the runtime has positively proved it missing;
  an unreachable runtime, a failed census, a busy or archived native and a lost
  launch answer are not absence.
- **Desk Adams** — an epic's or an ad-hoc topic's leadership session, placed in
  the `ADAM` desk workspace instead of an ECP when the Team Definition opts in
  with `desk_leadership`. The native runs in the desk; authority is derived from
  seat to subject to epic, so sharing a workspace never gives one subject's seat
  authority over another's work.
- **PR reviews** — one Adam tab and one `RSW` review room per pull request,
  identified by repository and number alone (never read back from a title) and
  recording the confirmed Jira key it was opened under.

See [durable desks](_docs/reference/2026-10-10-23-55-reference-kontord-durable-desks.md),
[desk seats](_docs/reference/2026-10-11-02-20-reference-kontord-durable-desk-seats.md),
[desk Adams](_docs/reference/2026-10-11-05-36-reference-kontord-desk-adams.md) and
[PR reviews](_docs/reference/2026-10-11-11-35-reference-kontord-pr-reviews.md).
They are merged through PRs #295–#300 (`733807cc`…`0971674d`, schema v131–v134).
The [ASMA-8454 report](_docs/reports/2026-10-11-16-00-report-asma-8454-desk-qualification.md)
records source and disposable-realm qualification only; Independent Review and
released-build acceptance are separate gates.

Backlog identity: Jira owns the full confirmed tracker key (`ASMA-8001`), which is
the public identifier; Kontor UUIDs remain internal. One project-scoped, exact
resolver maps a confirmed key to its epic or task; a malformed, missing,
unconfirmed, ambiguous or foreign key is a typed refusal, and public epic and
task selectors accept either the UUID or that exact key. Schema v95 retains
Jira's immutable issue id beside the mutable key, so a key change on the same
issue renames the binding in place while a readback naming a different issue is
an anti-rebind refusal. No numeric suffix, title or display code is ever
identity evidence.

Kontor also records one immutable, case-insensitively unique epic namespace per
project (`KOP`), allocated from title characters or selected manually. A native
item code (`KOP-8001`) is a display-only projection of that namespace and the
confirmed Jira numeric suffix; it is never persisted as a second Jira binding
and is never used to reconstruct one. Schema v72
preserves legacy codes as immutable evidence, quarantines duplicate or invalid
values, and allows a separate active assignment beside quarantined evidence.
Pre-v72 epics with no legacy code remain readable and operable; assignment is an
explicit write, and any topology needing the projection stays blocked until it
has happened.

Operational topology revision 1 remains byte-immutable. Revision 4's shipped
implementation opts into the typed `ITEM_CODE` token and centered-dot separator,
yielding historical names such as
`ESW · KOP-8001`, `ECP · KOP-8001` and `TSW · KOP-7869`. Projects and epics move
to it only through their existing preview/apply selection and upgrade seams. A
template asking for `ITEM_CODE` fails `placement_blocked` before any runtime
mutation unless the epic namespace is active and the relevant Jira binding has
exactly one confirmed readback.

Those v4 literals are implementation evidence, not the current naming
convention. The approved contract makes one pinned Team Definition JSON the
sole hierarchy/naming authority, with local seats such as `LSA`, `TPM`, `AUD`,
`SA`, `SEAT A`, `SEAT B`, `JUDGE`. `ADVISOR` is not a universal native seat
title; ASW seats use the exact configured registered professional role or
advisor-profile code, and ASW never performs Committee aggregation. Its
[2026-09-06 amendment](https://github.com/Carasent-ASMA/asma-modules/blob/master/_docs/ai-orchestration/architecture/2026-09-06-17-25-architecture-kontor-jira-key-identity.md)
makes the confirmed Jira key the recommended native scope (`ESW • ASMA-8001`,
`TSW • ASMA-7869`). The source renders it through typed `EPIC_JIRA_KEY`,
`TASK_JIRA_KEY` and `SCOPE_JIRA_KEY` tokens, resolved only from an exact
confirmed, project-scoped binding; a missing, ambiguous or non-canonical binding
refuses as `placement_blocked` before any native effect (PRs #215/#216,
`6355b850` / `f78d041e`). Item-code tokens, which the seeded operational fixture
still uses, keep their meaning for every definition pinned to them until an
explicit upgrade. See
[`docs/NATIVE_NAMING.md`](docs/NATIVE_NAMING.md), which records the 2026-09-02
live migration of one epic; other epics keep their historical pins.

Jira materialization recovery preserves the failed create batch as the sole
intent. Schema v74 adds an append-only recovery ledger keyed to its exact item,
ordinal and marker; the daemon may adopt an already-created Jira issue only when
project, issue kind, parent, summary, description and marker all match that
original create plan. Response items are mapped by ordinal, never by connector
array position, and recovery never moves a confirmed epic binding to another
issue.

Runtime permissions raised by a Committee filler remain runtime-owned, but the
authority to answer them is a Kontor control-plane effect. Inspection addresses
the exact Committee run, logical SeatBinding and attested native filler. Schema
v75 records the exact occupancy generation, native id, request id, UUIDv7
response identity and decision before dispatch; only runtime acknowledgement
advances it to confirmed. Confirmed calls replay without a second native effect,
while confirmation-unknown dispatches fail closed. The `leadership` MCP profile
contains only completion read/remediation, open-question read/record and these
exact Committee permission operations.

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

Model routing is data with one interpreter. A seat a fleet policy binds takes its
route from the policy, not from a template copy: `kontor-fleet` is the only
parser, validator, resolver and Committee allocator, and `kontor-fleet-activation`
is the only reader of an activated bundle — one atomically replaced activation
record naming immutable, content-addressed policy, Core Team roster and manifest
artifacts, all re-verified on every read. The daemon's placement path and the
daemon-free `kontor fleet-policy-resolve` read the same activated bytes through
that code, so a Paseo-direct consumer and Kontor-governed placement make the same
choice, with the same recorded reasons, for the same policy, key and stated
eligibility. Once an activation exists, a failed verification refuses placement
and never falls back to `fleet.yml` or template routing. The resolver chooses a
route; the scheduler's rules still decide whether that choice launches now,
waits or escalates. See [Configuration](docs/CONFIGURATION.md#activated-fleet-policy-asma-8280)
and the [live fleet routing decision](_docs/architecture/2026-09-24-20-45-architecture-live-fleet-routing.md)
(merged in PR #271 `7ffd650d` and PR #292 `4097afb8`; no live realm activation
is recorded here).

Quota headroom has one deliberate status caveat. The account-before-rung
resolver computes typed `Admit`, `Wait` and `NeedsHuman` outcomes, and launch
honours `Admit`. The delivery launch boundary still has to return a model rung,
however, so it currently drops the reset/escalation payload from `Wait` and
`NeedsHuman` and refuses the launch with a typed `ProviderHeadroom`
missing-evidence refusal before any native effect, rather than handing the
adapter a fallback route. Automatic pre-launch parking until the computed reset
is not shipped. The
source separately implements mid-run quota detection, durable
redacted handoff, successor placement and resident bounded reconciliation. It
does so only under explicit schema-v2 supervision enablement; schema v1 and an
absent or disabled policy remain inert. Those KON-OP-21 paths have local tests
and are merged through PR #170 (`080e2db3`, 2026-09-05). Deployment, live-runtime verification and realm enablement are separate receipt-backed stages.

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
When a fleet policy binds the seats, the shared allocator places the whole
Committee at once or not at all. A template that declares distinct vendors gets
reviewers that share none, and the formal Independent Review additionally seats
at most one Anthropic model and no route deeper than rung 2.

`planning_pair@1` (ASMA-8282, merged in PR #292) is a third, opt-in consultation
family, and it is advice rather than review: two independent read-only members
(`SEAT A`, `SEAT B`) frozen from one placement on distinct actual vendors, one
findings round in which neither sees the other's finding, at most one
clarification round, and the caller's own disposition, which must cite every
member's finding so dissent stays on the record. It has no Judge, quorum or
verdict, nothing it records has the shape a gate reads, and the caller selects
the protocol by name — Kontor never substitutes another. Only Independent
Review satisfies a formal review requirement. At this source the Paseo adapter
refuses every planning-pair member route, so a pair cannot yet be placed on
Paseo.

Consultation permissions and native session lifetime are distinct from the
verdict. Claude seats use an ordinary turn with a registry-bound tool guard;
Codex seats receive an explicit read-only sandbox. After durable settlement,
the bounded session-release scanner persists an exact native-identity intent,
archives through the runtime adapter, and confirms by readback. It never derives
settlement from an idle session. See [consultation lifecycle](docs/CONSULTATION_LIFECYCLE.md)
for startup configuration, recovery and rollback behavior.

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

Human interruption is the last safe step, not the first recovery action. Kontor
first performs deterministic inspection and policy-permitted automatic recovery,
including an eligible account, model or existing seat; then bounded Advisor and,
for genuinely cross-cutting ambiguity, Committee consultation; then bounded
evidence-backed remediation. If neither consultation establishes a safe,
authorized path, the work reaches `NeedsHuman`. A decision inherently reserved
to human authority goes directly there because an Advisor or Committee cannot
manufacture authority. The escalation is one prepared decision brief containing
the evidence, attempted paths, exact decision, options and no-action consequence
rather than repeated raw "blocked" notifications.

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
| Content-addressed policy activation | One atomically replaced pointer names immutable, hash-named policy, roster and manifest artifacts that the daemon and daemon-free readers verify identically | Reading a mutable checkout at placement time; a second policy interpreter |
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

Each registry row is one tool: name, minimum caller tier, execution, operation
kind and argument schema. Execution is one `/v1` route (method and path
template) for every tool but one: `kontor_fleet_policy_resolve` is the single
local operation, an in-process CLI handler with no route, admitted by the same
tier gate and argument validation, which MCP neither lists nor dispatches. The
JSON Schema a client is shown is derived from the same rows the dispatch path
validates against, with `additionalProperties: false` on every tool.

At `0971674d` the registry declares 230 tools: 93 `admin`, 76 `operator` and 61
`observer`. The CLI exposes all 230. Computed from source, an admin-tier MCP
server lists 228: the local operation, plus `kontor_account_profile_amend`,
which is kept off `tools/list` to save every seat's context but stays callable
by name. That figure is a source computation, not a live tool-discovery
observation.

Authority is the credential and nothing else. A realm has three secrets; a process
holds one and therefore *is* one seat (a hosted native seat holds a narrower,
seat-scoped bearer; see the security model). There is no per-call escalation argument
and no per-role policy inside `kontor-mcp` — running at two authorities means
running two servers.

| Tier | Reaches |
| --- | --- |
| `observer` | Reads, including session content |
| `operator` | Reads plus the work path: claim, settle, gate verdicts, session follow-up, intake, memory and experience proposals, consultation settlement, publication attestation and merge, desk-seat declaration and reconciliation, local fleet resolution |
| `admin` | Everything: project and account creation, epic apply, arming and disarming, selection correction, cutover attestation, gate waiver, memory approval, fleet policy and bundle publication and activation, launch-intent supersession |

Two refinements matter. A gate *waiver* is an authority-changing decision rather
than an ordinary verdict, so `kontor_gate_record` demands `admin` when its verdict
is `waived` — checked in the registry before the request exists and enforced again
by the daemon. And admin is not a domain bypass: revisions, idempotency,
admission, runtime capabilities, evaluator roles, evidence and closure gates are
enforced against it exactly as against a worker's credential.

A **serve profile** narrows presentation *and* the callable set within a tier, and
can never widen it. Profiles are declared in the registry beside the tier
declarations — deliberately not in a seat file, because a free-form tool list in
configuration would be a second authority model that drifts. Five are declared:
`worker` is the delivery surface a seat file or an OpenCode creation frame
selects; `consultation` is an advisor or committee filler's minimum, and is also
what the Paseo adapter composes into a Claude delivery seat's worktree
`.mcp.json` (`kontor-runtime-paseo/src/seat_mcp.rs`, `compose_for_seat`); `planning_pair_member` is a planning-pair member's
own run read plus its finding and answer; `planning_pair_caller` is the opt-in
caller surface that invokes a pair, asks its clarification and records the
disposition; `leadership` is completion read/remediation, open-question
read/record and exact Committee-seat permission inspection and response. An
unknown profile name refuses to start. See [`crates/kontor-mcp/seats/README.md`](crates/kontor-mcp/seats/README.md).

This is also the context-tax control: a seat is given the surface its role needs,
not the whole registry, so it spends its context on the work rather than on tool
schemas.

## Security model

- The MVP binds loopback only; wildcard/non-loopback addresses are refused.
- Every non-health route requires a realm bearer credential and a capability
  tier (`observer`, `operator` or `admin`).
- A hosted native seat receives a bearer derived from the operator secret and
  bound to its SeatBinding and occupancy generation. It carries no realm-wide
  tier, reaches only routes that explicitly require that seat, and is
  invalidated when the generation is fenced, without rotating realm
  credentials.
- External write identities are narrow and optional. The GitHub App gateway
  expects an App granted only `checks: write`, `pull_requests: read` and
  `contents: write`; its private key is read from a path `config/github-app.json`
  names, never from the document, absent configuration leaves it inert, and a
  present document that does not parse refuses the start. The Jira credential lives
  in the platform keychain under an address scoped to the immutable realm and
  canonical state root, so a copied realm cannot overwrite another root's entry.
- Host and Origin are validated before route handling.
- Secrets are references; DTOs, domain rows, logs, exports and process arguments
  have no field for secret values.
- Each state root has one exclusive daemon lock, database, credential file and
  immutable realm ID.
- Clients never receive runtime endpoints or provider credentials.
- A published fleet policy names accounts, models and seats but never locates
  or authenticates them: preview and publication refuse credential material,
  filesystem paths and email addresses.
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

- `kontor-core`, `kontor-store`: durable truth and transactions — domain
  identifiers, states, commands and repository ports; SQLite repositories,
  ordered migrations and event append/replay;
- `kontor-runtime`, `kontor-runtime-paseo`, `kontor-runtime-ao`,
  `kontor-runtime-codex`: replaceable native execution — the adapter contract,
  the Paseo adapter, and the AO and narrow Codex adapters the daemon's runtime
  configuration still refuses;
- `kontor-context`, `kontor-accounts`, `kontor-teams`, `kontor-profiles`:
  immutable execution inputs — Context Packs and portable handoffs, non-secret
  account profiles and routing, team templates and run snapshots, work profiles
  and seed packs;
- `kontor-fleet`, `kontor-fleet-activation`: the one fleet policy parser,
  validator, route resolver and Committee allocator, and the read-only,
  daemon-free reader of one activated policy bundle shared by daemon placement
  and direct-mode resolution;
- `kontor-scheduler`, `kontor-policy`, `kontor-calendar`: admission, safety and
  epic-completion compilation;
- `kontor-jira`, `kontor-intake`, `kontor-memory-cognee`: external boundaries —
  the native Jira connector, the durable event-intake seam, and the bounded,
  candidate-only Cognee transport for derived experience projections;
- `kontor-api`, `kontor-daemon`, `kontor-cli`, `kontor-mcp`: one public control
  contract, one tool registry and its transports; the daemon is the composition
  root and hosts the GitHub App publication gateway;
- `apps/*`: projections over the public API, never independent truth. The
  console's Tokenomics view only frames separately generated report data and
  needs no realm connection.

## Explicit non-goals for the MVP

- implementing an LLM, coding harness, terminal multiplexer or provider login;
- replacing Paseo, Agent Orchestrator, Codex, AgentsRoom or Jira;
- replacing GitHub or recreating Jira, GitHub or runtime general-purpose UI;
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
- [Kontor mission-control and attention plan](https://github.com/Carasent-ASMA/asma-modules/blob/master/_docs/ai-orchestration/plans/2026-08-18-17-17-plan-kontor-operator-surface-and-attention.md)
- [Confirmed Jira-key identity](https://github.com/Carasent-ASMA/asma-modules/blob/master/_docs/ai-orchestration/architecture/2026-09-06-17-25-architecture-kontor-jira-key-identity.md) — public identity and native scope from the confirmed key
- [Kontor current workflow contracts](https://github.com/Carasent-ASMA/asma-modules/blob/master/_docs/ai-orchestration/reference/2026-09-27-08-11-reference-kontor-current-workflow-contracts.md) — the adopted workflow rules, separated from trials and proposals

Those documents govern ASMA-specific rollout. This file governs the public
repository boundary and must stay readable without the parent checkout.
Repository-local detail lives in [`_docs/`](_docs/index.md): durable desks and
their seats, Adams and PR reviews under `_docs/reference/`, the live fleet routing
and local verification decisions under `_docs/architecture/`.
