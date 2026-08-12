# ASMA Kontor

[![CI](https://github.com/Carasent-ASMA/asma-rs-kontor/actions/workflows/ci.yml/badge.svg)](https://github.com/Carasent-ASMA/asma-rs-kontor/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license-intent)

Kontor is a local-first control plane for durable, policy-governed work across
AI-agent runtimes. It keeps the project plan, dependencies, work profiles,
team/seat bindings, command receipts, evidence and external-workflow state in
one place while Paseo, Agent Orchestrator, Codex and future adapters continue
to own their native sessions and provider mechanics.

> [!CAUTION]
> Kontor is pre-1.0 and under active MVP development. The current `master`
> implements the core control plane and primary clients, but the full pilot,
> backup/security close-out, calendar engine and source-event intake are not all
> complete. Do not treat it as a production-ready autonomous scheduler yet.

## Why does another orchestration project need to exist?

There are many capable agent orchestrators. They solve **execution**: starting a
model, managing a terminal, streaming output, asking for permissions and
keeping a native conversation alive. Kontor does not rebuild that layer.

The unsolved problem in a long-running ASMA mini-project is **durable control**:

- Which task is actually ready, armed and safe to start?
- Which work profile, team template, account and runtime were pinned?
- Did a launch happen before a crash, or is retrying safe?
- Is a missing runtime session failed, stale, orphaned or simply unreachable?
- Does one role slot already have a live session?
- Is Jira aligned with the internal phase and gate evidence?
- Can the same project continue through another runtime without inventing a
  second source of truth?

An execution runtime cannot answer those questions authoritatively for every
other runtime, and asking each orchestrator to grow a project database,
scheduler, policy engine and Jira model would create several competing control
planes. Kontor is the deliberately small authority above them.

| Concern | Kontor owns | Agent runtime owns |
| --- | --- | --- |
| Project truth | Tasks, dependencies, profiles, gates, evidence, policy | No |
| Scheduling | Deterministic admission, leases, budgets and explanations | Native execution only |
| Effects | Durable intent, idempotency and confirmation receipts | The actual native effect |
| Sessions | Binding, freshness and derived safe status | Process, transcript, tools and provider auth |
| Recovery | Reconciliation and explicit uncertainty | Native inventory and inspection |
| Integrations | Typed projections and delegated commands | Runtime-specific protocol |

See [Architecture](ARCHITECTURE.md) for the complete boundary and technology
rationale.

## What Kontor provides

The current repository includes:

- a versioned domain model for realms, projects, tasks, phase/gate workflows,
  team runs, agent runs and external-ticket projections;
- a crash-recoverable SQLite store with migrations, append-only events,
  idempotent command receipts and desired/observed/derived state;
- composable work profiles, immutable team snapshots, persistent role seats,
  portable Context Packs and redacted handoffs;
- a shared runtime contract with capability/trust grades, atomic launch
  admission, workspace binding, reconciliation and gap-safe session timelines;
- adapters for Paseo, Agent Orchestrator and a narrow direct Codex fallback;
- deterministic scheduling, collision-safe leases, guardrail evidence and
  bounded parked-work recovery;
- non-secret provider-account profiles plus delegated `asma fleet` and
  `asma jira sync` integration boundaries;
- an authenticated loopback daemon with a versioned HTTP/SSE API and checked-in
  OpenAPI contract;
- one capability catalogue exposed consistently through the `kontor` CLI and
  stdio MCP server;
- a responsive React operator console and Tauri desktop shell.

Still open in the MVP are the full disposable end-to-end proof, final
backup/export/security hardening, calendar/holiday admission, durable
source-event intake and epic close-out. The code and documentation should say
“unsupported” or “unconfigured” for an unfinished capability rather than
pretending it exists.

## How this helps ASMA

Kontor turns the conventions ASMA currently enforces through instructions,
backlog reconciliation and careful human supervision into inspectable system
rules:

- one view of a mini-project from plan through implementation and verification;
- no duplicate persistent role session for one declared team slot;
- safe parallel work only when module/worktree isolation is proven;
- explicit provider-account selection without storing secrets in project data;
- no false success when a runtime disappears or an event stream has a gap;
- deterministic Jira field/status/ownership convergence through the existing
  ASMA CLI rather than direct store writes;
- durable evidence that survives daemon, runtime and model changes;
- the same realm-qualified facts for CLI, MCP, desktop and phone-width clients.

Kontor is designed for coding, research, architecture, UX/UI, QA, operations
and incident work described by data. Those are profile packs, not hard-coded
branches in the scheduler.

## Architecture in 30 seconds

```text
CLI / MCP / responsive console / Tauri desktop
                    |
          authenticated loopback API
                    |
              kontor-daemon
        +-----------+-----------+
        |           |           |
   domain/store  scheduler   policy/evidence
        |           |           |
        +----- runtime contract-+
               /      |      \
           Paseo      AO     Codex
              native agent sessions

      delegated subprocess boundaries
          asma fleet   asma jira sync
```

Three rules shape the design:

1. **One writer per fact.** Kontor never edits another tool's internal store.
2. **Uncertainty is not completion.** Stale, divergent, unavailable and
   orphaned are visible states, never guessed terminal outcomes.
3. **Intent precedes effects.** Runtime-changing commands are durably recorded
   before dispatch and confirmed through correlated evidence.

## Try the local control plane

### Prerequisites

- Rust from [`rust-toolchain.toml`](rust-toolchain.toml) (`1.97.1`, including
  rustfmt and clippy);
- Node.js `>=24` and pnpm `11.6.0` for the console/desktop packages;
- Tauri 2 platform prerequisites when building the desktop shell.

Build and start an empty local realm:

```sh
cargo build --workspace
cargo run -p kontor-daemon -- --state-root /tmp/kontor-demo
```

In another terminal, use the same state root. The CLI reads the daemon endpoint
and the least-privileged local credential required for each operation:

```sh
cargo run -p kontor-cli -- --state-root /tmp/kontor-demo health
cargo run -p kontor-cli -- --state-root /tmp/kontor-demo realm show
cargo run -p kontor-cli -- --state-root /tmp/kontor-demo runtime list
cargo run -p kontor-cli -- --help
```

A realm with no `runtimes.json` is valid for inspecting the control plane; its
session operations report that no runtime is configured. Runtime configuration
is intentionally explicit and remains local to the state root.

For frontend development:

```sh
pnpm install --frozen-lockfile
pnpm --filter kontor-console dev
```

The daemon binds loopback only. There is intentionally no “listen on every
interface” development shortcut.

## Repository map

| Path | Responsibility |
| --- | --- |
| `crates/kontor-core` | Domain identifiers, lifecycle, specs and ticket policy |
| `crates/kontor-store` | SQLite durability, migrations, events, receipts and queries |
| `crates/kontor-runtime*` | Shared runtime contract and Paseo/AO/Codex adapters |
| `crates/kontor-context` | Context Pack resolution, redaction and handoff |
| `crates/kontor-accounts` | Non-secret account profiles and launch routing |
| `crates/kontor-teams`, `kontor-profiles` | Versioned teams, seats, workflows and seed packs |
| `crates/kontor-scheduler`, `kontor-policy`, `kontor-calendar` | Admission, leases, guardrails and time policy |
| `crates/kontor-integrations-asma`, `kontor-intake` | Delegated ASMA integrations and event intake |
| `crates/kontor-api`, `kontor-daemon` | Authenticated loopback contract and composition root |
| `crates/kontor-cli`, `kontor-mcp` | Human and agent control surfaces over one operation catalogue |
| `apps/console`, `apps/desktop` | Responsive console and Tauri shell |
| `tests/contract`, `tests/e2e` | Cross-adapter contracts and full-system proof |

## Development and contribution

Start with [Contributing](CONTRIBUTING.md). Architectural changes should also
read [Architecture](ARCHITECTURE.md); security findings follow
[Security](SECURITY.md), and participation follows the
[Code of Conduct](CODE_OF_CONDUCT.md).

The standard local gates are:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo audit
cargo deny check
pnpm install --frozen-lockfile
pnpm -r typecheck
pnpm -r test
pnpm audit --prod
```

`scripts/verify-tree.py` additionally proves that a staged or committed export
works without hidden working-tree state and that `Cargo.lock` is reproducible.

## License intent

Kontor source is offered under **MIT OR Apache-2.0**, at the user's option. The
intent is permissive reuse in open-source and commercial environments while
the Apache-2.0 option provides an explicit patent grant. See
[LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE),
[NOTICE](NOTICE), [Provenance](PROVENANCE.md) and
[Third-party notices](THIRD_PARTY_NOTICES.md).

Paseo, Agent Orchestrator, Codex and the `asma` CLI are separate installations;
their code and licenses are not included here. Public release remains subject
to the reviews listed in [Provenance](PROVENANCE.md).

## FAQ

### Is Kontor itself an agent runtime?

No. It does not implement models, provider login, PTYs or native transcripts.
It coordinates supported runtimes through adapters.

### Does Kontor replace Paseo or Agent Orchestrator?

No. They remain execution planes. Kontor supplies durable cross-runtime state,
policy, scheduling, reconciliation and evidence above them.

### Does Kontor replace AgentsRoom or Jira?

No. AgentsRoom remains the interim backlog/knowledge surface during the MVP,
and Jira remains the external workflow system. Kontor owns its internal facts
and delegates supported synchronization instead of editing either store.

### Does Kontor store agent transcripts or credentials?

No transcript/token copy is part of Kontor domain state. Session content stays
runtime-owned and is brokered through the daemon. Account records contain
non-secret metadata and credential references, never secret values.

### Is Kontor a cloud service or multi-user server?

Not in the MVP. One daemon owns one local realm and binds loopback only. Remote
access, realm federation, multi-host workers and native mobile pairing require
separate security and identity decisions.

### How do I add another runtime?

Implement the shared `RuntimeAdapter`, declare only capabilities the runtime
can prove, add recorded contract fixtures, and pass the common consistency and
mutation tests. See [Architecture](ARCHITECTURE.md#runtime-extension-contract).

### Where is the detailed MVP design?

The concise repository contract is [Architecture](ARCHITECTURE.md). The full
ASMA implementation design and active plan live in the parent `asma-modules`
documentation for epic `ASMA-7744`; links are collected at the end of that
document.
