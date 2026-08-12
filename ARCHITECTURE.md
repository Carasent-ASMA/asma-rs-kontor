# ASMA Kontor Architecture

> **Status:** Current pre-1.0 repository contract
>
> **Scope:** Authority boundaries, consistency model, technology choices and
> extension rules for `asma-rs-kontor`.

## Purpose

Kontor is the durable project control plane above replaceable agent execution
runtimes. It supervises work that may outlive one process, one model, one
provider account or one orchestration product.

It exists because execution and control have different lifecycles:

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
             Paseo | AO | Codex | future
                       |
                native sessions

Separate supported commands: asma fleet | asma jira sync
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
| Provider capacity/cooldown mechanics | `asma fleet` |
| External ticket workflow | Jira through `asma jira sync` |
| AgentsRoom backlog/memory during the MVP | AgentsRoom |

The core rule is **one writer per fact**. Kontor uses supported adapter or
subprocess interfaces and never edits Paseo, AO, fleet, Jira or AgentsRoom
internal stores.

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

For Paseo, one Jira epic maps to one project; each ticket maps to one PASE
(Paseo Agent Session Environment), and its persistent role seats live inside
that PASE. The Git worktree is separate checkout/isolation evidence, not the UI
workspace identity.

## Deterministic scheduling and policy

A model may recommend work but cannot write scheduler truth. Admission is a
deterministic reduction over dependencies, serialization/module collisions,
verified worktree leases, runtime freshness/capabilities, provider/account
health, budgets, permissions, explicit execution authorization and optional
calendar policy.

Every refusal is explainable. Lowering an adaptive admission window stops new
work but does not cancel already bounded work. Gate authority and recovery
budgets come from immutable work-profile/team snapshots, not role-name guesses.

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
| `asma` subprocess delegation | Reuse fleet/Jira mechanisms and their existing ownership | Writing fleet/Jira stores or duplicating their clients |
| Exact dependency pins + lockfiles | Reproducible pre-1.0 builds and auditable license/advisory state | Floating dependency resolution |
| `MIT OR Apache-2.0` | Permissive reuse plus an explicit Apache patent grant | A custom source-available license |

Storage and runtime implementations are replaceable behind contracts. A new
choice must first prove parity for transactions, migrations, receipts,
reconciliation, backup and continuity; novelty alone is not a reason to move a
control-plane boundary.

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
- `kontor-scheduler`, `kontor-policy`, `kontor-calendar`: admission and safety;
- `kontor-integrations-asma`, `kontor-intake`: external boundaries;
- `kontor-api`, `kontor-daemon`, `kontor-cli`, `kontor-mcp`: one public control
  contract and its transports;
- `apps/*`: projections over the public API, never independent truth.

## Explicit non-goals for the MVP

- implementing an LLM, coding harness, terminal multiplexer or provider login;
- replacing Paseo, Agent Orchestrator, Codex, AgentsRoom, Jira or `asma fleet`;
- storing a duplicate native transcript/token stream;
- remote bind, multi-host workers, multi-user tenancy or realm federation;
- automatic task decomposition with model-written scheduler state;
- a hard-coded catalogue of work types or one universal agent team.

## Detailed ASMA design

The repository contract above is intentionally concise. The full implementation
baseline and active epic plan are maintained in the parent polyrepo:

- [Kontor MVP control-plane architecture](https://github.com/Carasent-ASMA/asma-modules/blob/master/_docs/ai-orchestration/architecture/2026-08-08-20-12-architecture-asma-kontor-control-plane.md)
- [Kontor MVP implementation plan](https://github.com/Carasent-ASMA/asma-modules/blob/master/_docs/ai-orchestration/plans/2026-08-08-20-12-plan-asma-kontor-mvp-control-plane.md)

Those documents govern ASMA-specific rollout. This file governs the public
repository boundary and must stay readable without the parent checkout.
