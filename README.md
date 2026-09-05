# ASMA Kontor

> Implementation counts and delivery claims are stamped to `082b63ad` (2026-09-05). See the [canonical implementation inventory](https://github.com/Carasent-ASMA/asma-modules/blob/master/_docs/ai-orchestration/reference/2026-09-05-11-36-reference-kontor-implementation-inventory.md). Registry membership, MCP advertising and credential authorization are distinct.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license-intent)

Kontor is a local-first control plane for durable, policy-governed work across
AI-agent runtimes. It keeps the project plan, dependencies, work profiles,
team/seat bindings, command receipts, evidence, approved memory and
external-workflow state in one place while Paseo and future adapters continue to
own their native sessions and provider mechanics.

It exists to serve two things:

- **Autonomy** — work proceeds without an operator driving each step, on
  deterministic rails the kernel executes from versioned data, carrying that
  operator's own approved decisions forward as durable memory instead of
  re-learning them every session.
- **Delivery quality** — the unit of value is a *completed* piece of work.
  Completion is defined, checked and independently verified: implemented,
  tested, reviewed by an authority that did not do the work, integrated,
  released and evidenced. **A fully green task graph does not complete an
  epic**; only a compliant independent verdict plus complete closeout evidence
  does.

> [!CAUTION]
> Kontor is pre-1.0 and under active development. `master` runs real
> multi-repository delivery daily, but some designed capabilities are
> deliberately unreachable rather than half-built — see
> [What is not built](#what-is-not-built). Nothing here invents behaviour it
> cannot prove: an unfinished capability says "unsupported" or "unconfigured".

## Why does another orchestration project need to exist?

There are many capable agent orchestrators. They solve **execution**: starting a
model, managing a terminal, streaming output, asking for permissions and
keeping a native conversation alive. Kontor does not rebuild that layer.

The unsolved problem in a long-running ASMA mini-project is **durable control**:

- Which task is actually ready and safe to start, and what is narrowing it?
- Which work profile, team template, account and runtime were pinned?
- Did a launch happen before a crash, or is retrying safe?
- Is a missing runtime session failed, stale, orphaned or simply unreachable?
- Does one role slot already have a live session?
- Is Jira aligned with the internal phase and gate evidence?
- Can the same project continue through another runtime without inventing a
  second source of truth?
- Is this epic *finished*, or merely green?

An execution runtime cannot answer those questions authoritatively for every
other runtime, and asking each orchestrator to grow a project database,
scheduler, policy engine and Jira model would create several competing control
planes. Kontor is the deliberately small authority above them.

Here, **small means a narrow authority boundary**. It does not mean a small
codebase or a weak UI. The operator still needs a rich mission-control dashboard
for the truth no single connected system can show: dependency order, what is
running or waiting, what needs attention, what can run next and whether the work
is actually delivered.

| Concern | Kontor owns | Agent runtime owns |
| --- | --- | --- |
| Project truth | Tasks, dependencies, profiles, gates, evidence, policy | No |
| Scheduling | Deterministic admission, leases, budgets and explanations | Native execution only |
| Effects | Durable intent, idempotency and confirmation receipts | The actual native effect |
| Sessions | Binding, freshness and derived safe status | Process, transcript, tools and provider auth |
| Recovery | Reconciliation and explicit uncertainty | Native inventory and inspection |
| Integrations | Typed projections and native connector commands | Runtime-specific protocol |

The reuse rule is simple:

| Existing system | Keep using it for | Kontor adds |
| --- | --- | --- |
| Jira | Business workflow, issue fields and team-established process | Deterministic orchestration and convergence with execution evidence |
| GitHub | Repositories, pull requests, checks and releases | Joined delivery evidence and completion truth |
| Paseo or another runtime | Native sessions, transcripts, tools, permissions and provider login | Durable bindings, safe derived state and cross-runtime recovery |

Kontor may absorb one narrow capability only when keeping or adopting a whole
external product for it would cost more over its lifetime in dependencies,
synchronization and split authority. That requires a written reason and a clean
cutover to one writer. It is not permission to recreate a mature product.

See [Architecture](ARCHITECTURE.md) for the complete boundary and technology
rationale.

The [recommended teams and seats](_docs/reference/2026-09-04-23-35-reference-recommended-teams-and-seats.md)
describe role responsibilities and fleet-design tradeoffs, with implementation
limits separated from recommendations.

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
- a production Paseo adapter, plus hermetic Agent Orchestrator and direct-Codex
  adapters that keep the shared contract honest without being composable in a
  release build;
- a generic, versioned **session topology** — epic control plane, ticket
  workspaces, consultation workspaces — published per project rather than
  hard-coded;
- immutable project-scoped **epic backlog codes** with deterministic
  collision expansion and manual overrides, plus display-only item codes such
  as `KOP-8001` derived from confirmed Jira numeric identities;
- a deterministic **standard role catalogue** (56 roles across 9 segments) with
  typed role-code seat selection and optional display labels;
- **Project Core Teams**, quick ad-hoc sessions and preview/apply promotion of an
  ad-hoc session into a real epic with a durable handoff;
- read-only **Advisors** and **Committees** with frozen inputs, independent
  findings, preserved dissent and typed aggregate verdicts;
- configurable **epic Completion Profiles** that compile integration, independent
  verdict, bounded remediation and six closeout receipts into ordinary task,
  team and gate machinery;
- deterministic default-allow scheduling, collision-safe leases, guardrail
  evidence and bounded parked-work recovery;
- native connectors for Jira and for live provider-quota observation, with
  routing by admissible account and rung;
- non-secret provider-account profiles with one credential home per account;
- an authenticated loopback daemon with a versioned HTTP/SSE API and checked-in
  OpenAPI contract;
- one capability catalogue — 166 registry operations, 165 advertised at admin MCP scope in the 2026-09-05 snapshot — exposed through the stdio MCP server and
  a `kontor` CLI *generated from the same registry*, with credential tiers and
  narrow serve profiles;
- a responsive React operator console and Tauri desktop shell.

### What is not built

Stated plainly, because a README that implies a capability is worse than one that
admits its absence:

| Not built | Detail |
| --- | --- |
| Calendar admission | `kontor-calendar` implements windows, holiday import and drain with tests, and is reached by **no** route and **no** tool. Every project resolves to `unrestricted`. |
| Post-delivery profile packs | The seed manifest declares 17 work-profile categories; **four** ship (`code`, `ux-ui-layout`, `research`, `docs`). Operations, incident response, maintenance, compliance and retirement are declared, not implemented. |
| KON-OP-21 release verification | KON-OP-21 is implemented and merged through PR #170 (`080e2db3`, 2026-09-05), included in the inspected release `082b63ad`. Local contract coverage is recorded; independent qualification, coherent fleet deployment and realm enablement require their own receipts and are not certified by this documentation refresh. |
| Launch-time quota waiting | The account-before-rung resolver computes `Wait` / `NeedsHuman`, but delivery launch still needs a model rung and preserves the adapter's typed refusal path instead of parking until the computed reset. |
| Automatic stale-evidence rejection | Post-freeze product drift that invalidates a completion bundle is currently caught by a reviewer, not by the state machine. |
| Unified mission-control dashboard | Individual console views do not yet provide one joined dependency graph, running/waiting/blocked/stale state, next eligible work, prepared operator decisions and delivery evidence. |
| Remote access and federation | Loopback only. Multi-realm switching, remote bind, pairing and TLS are unbuilt security designs, not hidden flags. |
| AO and direct-Codex in production | `ao` and `codex` are a closed deferred list; a configuration naming either is refused rather than falling back to Paseo. |

## How this helps ASMA

Kontor turns the conventions ASMA currently enforces through instructions,
backlog reconciliation and careful human supervision into inspectable system
rules:

- durable joined truth for one mission-control view from plan through implementation and verification; the unified dashboard itself is still listed above as unbuilt;
- no duplicate persistent role session for one declared team slot;
- safe parallel work only when module/worktree isolation is proven;
- explicit provider-account selection without storing secrets in project data;
- no false success when a runtime disappears or an event stream has a gap;
- deterministic Jira field/status/ownership convergence through a native
  connector rather than direct store writes;
- durable evidence that survives daemon, runtime and model changes;
- the same realm-qualified facts for CLI, MCP, desktop and phone-width clients;
- an epic that cannot be declared finished on its own say-so.

Kontor is designed for coding, research, architecture, UX/UI, QA, operations
and incident work described by data. Those are profile packs, not hard-coded
branches in the scheduler.

## Architecture in 30 seconds

```text
CLI / MCP / responsive console / Tauri desktop
                    |
      authenticated loopback API  (/v1, bearer + tier)
                    |
              kontor-daemon
        +-----------+-----------+
        |           |           |
   domain/store  scheduler   policy/evidence
        |           |        + completion
        +----- runtime contract-+
                    |
                  Paseo            (ao | codex refused)
              native agent sessions

        native connectors inside the daemon
              Jira    provider usage
```

Six rules shape the design:

1. **One writer per fact.** Kontor never edits another tool's internal store,
   and never invokes the `asma` CLI.
2. **Uncertainty is not completion.** Stale, divergent, unavailable and
   orphaned are visible states, never guessed terminal outcomes.
3. **Intent precedes effects.** Runtime-changing commands are durably recorded
   before dispatch and confirmed through correlated evidence.
4. **The workflow is data.** No core code branches on a profile id, a role name,
   a topology kind or a work type. Adding a work type is publishing a spec.
5. **Proposal is not authority.** A model may recommend anything and submit
   evidence; only a deterministic evaluator reading a pinned snapshot writes
   scheduler truth, gate verdicts or receipts.
6. **A green graph is not a delivered epic.** Completion runs its pinned profile
   to an independent verdict and six closeout receipts, or it does not complete.

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

In another terminal, use the same state root. One command is one tool is one
`/v1` operation: the command tree is generated from the MCP registry, so `kontor
epic-apply` and the `kontor_epic_apply` tool are the same operation with the same
arguments. The process holds exactly one credential tier and defaults to
`observer`, so a command that mutates has to ask:

```sh
cargo run -p kontor-cli -- --help                                  # every tool, as a command
cargo run -p kontor-cli -- --state-root /tmp/kontor-demo realm-get
cargo run -p kontor-cli -- --state-root /tmp/kontor-demo projects-list
cargo run -p kontor-cli -- --state-root /tmp/kontor-demo runtime-capabilities-list
cargo run -p kontor-cli -- --state-root /tmp/kontor-demo --tier admin project-ensure \
    --idempotency-key <key> --name demo --root-path /tmp/demo-repo
```

`--state-root` has no default: it names the realm to act on and holds the
credential file the CLI reads. `--base-url` is only needed when the realm is not
on its default loopback port. Every write takes an `idempotency_key` you choose;
repeating one returns the original receipt rather than recording a second command.

A realm with no `runtimes.json` is valid for inspecting the control plane; its
session operations report that no runtime is configured. Runtime configuration
is intentionally explicit and remains local to the state root.

Reading and writing are also separate processes on the MCP side: a server runs at
one credential tier and optionally one narrow serve profile, so a delivery seat
is given the narrow tools it works with rather than the full advertised admin surface. See
[`crates/kontor-mcp/seats/README.md`](crates/kontor-mcp/seats/README.md).

Seat lifecycle policy has a checked example
[`supervision.yml`](config/examples/paseo-supervision.yml). Copying it into the
state root opts into the resident succession engine only when the
document is schema v2, `watchdog.enabled` is `true`, and
`recovery.max_concurrent_successions` is explicitly set inside the safe bound.
With no policy, with schema v1, or with a disabled watchdog, automatic
succession does not start and Kontor invents no cadence or capacity. See
[Configuration](docs/CONFIGURATION.md).
This path is merged at `080e2db3` (PR #170, 2026-09-05). Independent audit, live-runtime verification and per-realm enablement require separate receipts.

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
| `crates/kontor-runtime*` | Shared runtime contract; production Paseo adapter; hermetic AO and Codex adapters |
| `crates/kontor-context` | Context Pack resolution, redaction and handoff |
| `crates/kontor-accounts` | Non-secret account profiles and launch routing |
| `crates/kontor-teams`, `kontor-profiles` | Versioned teams, seats, workflows and seed packs |
| `crates/kontor-scheduler`, `kontor-policy`, `kontor-calendar` | Admission, leases, guardrails, epic-completion compilation and (unexposed) time policy |
| `crates/kontor-jira`, `kontor-intake` | Native Jira connector and durable event intake |
| `crates/kontor-api`, `kontor-daemon` | Authenticated loopback contract and composition root |
| `crates/kontor-cli`, `kontor-mcp` | One tool registry; the MCP server serves it and the CLI is generated from it |
| `apps/console`, `apps/desktop` | Responsive console and Tauri shell |
| `tests/contract`, `tests/e2e` | Cross-adapter contracts and full-system proof |

## Development and contribution

Start with [Contributing](CONTRIBUTING.md). Architectural changes should also
read [Architecture](ARCHITECTURE.md); security findings follow
[Security](SECURITY.md), and participation follows the
[Code of Conduct](CODE_OF_CONDUCT.md).

Kontor deliberately does not run CI in GitHub Actions. Verification runs
locally against the exact candidate commit before merge and its results are
recorded in the pull request or owning delivery evidence. This is an explicit,
reversible repository policy; re-enabling GitHub-hosted CI requires an explicit
decision and documentation change. See the
[local verification policy](_docs/architecture/2026-09-01-13-25-architecture-kontor-local-verification-policy.md).

The required local gates are:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo audit
cargo deny check
pnpm install --frozen-lockfile
pnpm --filter kontor-console verify:api
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

## The Foundation acceptance proof (KON-MVP-18)

> This proof accepted the Foundation control plane and **passed**; the retained
> bundle is the record. It is kept as a regression contract rather than as the
> current acceptance boundary — delivered work is now accepted by an epic
> Completion Profile, not by this test.

One command runs the whole disposable mini-project proof and writes the evidence
bundle that accepted the Foundation MVP:

```sh
cargo test -p kontor-tests-e2e --test pilot -- --nocapture
```

It is deterministic and offline — two scripted runtimes, no socket, no child
process, no clock. Each run writes an ephemeral bundle to
`target/kontor-pilot/<run-id>/` and a retained one to
`docs/evidence/KON-MVP-18/<run-id>/`; start at `REPORT.md`, which maps every
acceptance criterion to the artifacts backing it, and `verdict.json`, which
carries the overall accept/reject. The run id is derived from the commit and the
fixture digests, so rerunning an unchanged tree reproduces the same bundle
rather than accumulating one directory per invocation.

Every criterion is registered up front, so a criterion the driver never answers
is recorded `missing` and rejects the run on its own — an unrun case cannot read
as a passing one. A case blocked on a seam that has not merged names the ticket
that owns it and rejects the bundle without failing the gate; a case that
*fails* is a defect in the merged tree, and fails the gate.

`manifest.json` is written last and carries a SHA-256 of every other file in the
bundle plus a combined `root_hash`, so a retained bundle can be checked rather
than taken on faith; it names its own exclusion (`manifest.json` cannot hash
itself) and the command to recompute. It also lists `unlinked_artifacts` — files
no case cites — because an artifact that exists without a case pointing at it is
evidence nobody will read.

Because the run id keys off the commit, running the pilot on a tree whose HEAD
has moved writes a *new* `docs/evidence/KON-MVP-18/<run-id>/` directory rather
than rewriting the committed one. That is deliberate — evidence belongs to the
tree that produced it, and the retained bundle's `commit` field names that tree
— but it does mean a run after an unrelated commit leaves an untracked
directory. Retain it only if it is the acceptance record you mean to publish;
otherwise delete it.

Verifying the installed harnesses is a separate, opt-in question:

```sh
KONTOR_PILOT_LIVE=1 cargo test -p kontor-tests-e2e --test pilot_live -- --nocapture
```

Without the variable it asserts nothing and says so, because a probe that passes
silently when a runtime is absent teaches an operator to read green as "the live
harness works".

## Dependency policy (CON-007)

Paseo, Agent Orchestrator and Codex are separate installations; their code and
licenses are not included here. Public release remains subject
to the reviews listed in [Provenance](PROVENANCE.md).

## Backup, restore, export and security

Operator runbook: [`RECOVERY.md`](RECOVERY.md). In short:

```sh
kontor-daemon --state-root <dir> snapshot          # VACUUM INTO, verified, pruned to 7
kontor-daemon --state-root <dir> export --out f    # versioned, redacted, deterministic JSON
kontor-daemon --state-root <dir> restore --snapshot f   # same realm only, realm stopped
kontor-daemon --state-root <dir> import --from f --project <id>   # a *different* realm
kill -HUP <pid>                                    # rotate a serving realm's credentials
```

Snapshot and export are safe while the realm serves; restore, import and the
stopped-realm rotation take the state root's exclusive lock. A restore never
overwrites a *different* initialized realm, and a restored realm keeps
scheduling shut until it has reconciled.

## License and provenance

### Is Kontor itself an agent runtime?

No. It does not implement models, provider login, PTYs or native transcripts.
It coordinates supported runtimes through adapters.

### Does Kontor replace Paseo or Agent Orchestrator?

No. They remain execution planes. Kontor supplies durable cross-runtime state,
policy, scheduling, reconciliation and evidence above them.

### Does Kontor replace AgentsRoom, Jira or GitHub?

GitHub remains the repository, pull-request, check and release system. Kontor
uses those observations as delivery evidence.

Jira remains the external workflow system; Kontor converges to it through a
native connector and never edits its store. Jira issue keys remain their native
full values, such as `ASMA-8001`; Kontor does not attempt to create keys like
`ASMA-ESW-8001` or reconstruct Jira identity from a display name.

Kontor separately assigns each epic one immutable namespace inside its project,
for example `KOP`. Omission derives a candidate from title initials and expands
unused title characters until it is unique; an operator may instead supply a
manual code. Once Jira readback confirms `ASMA-8001` for the epic and
`ASMA-7869` for a task, item codes are `KOP-8001` and `KOP-7869`. Those item
codes are projections, not tracker identities. The shipped topology-v4
centered-dot rendering is historical implementation behavior; the approved
current contract puts all hierarchy and naming in pinned Team Definition JSON
and recommends `ESW • KOP-8001` and `TSW • KOP-7869`. See
[`docs/NATIVE_NAMING.md`](docs/NATIVE_NAMING.md). A
missing, malformed or ambiguous confirmed Jira binding blocks placement rather
than falling back to a title, UUID or imported short code. Existing native
objects keep their historical names and Team Definition pin until an explicit,
identity-preserving `team-definition:upgrade-preview` / `:upgrade-apply`
completes exact readback.

Confirmed Jira tasks and epics are reconciled automatically by the resident
daemon controller. It runs once after the startup barrier opens, after durable
Kontor changes, and at a 30-second recovery backstop. Each subject must select
one exact installed immutable workflow revision for its entity kind and frozen
work profile; missing or mismatched configuration fails closed. Kontor records
contradictory external state as a durable conflict for explicit resolution and
does not spin on unchanged conflicts or failed Jira effects. See the
configuration guide's [Jira reconciliation](docs/CONFIGURATION.md#jira-reconciliation)
section.

AgentsRoom is being replaced per project, per subject. Write authority for a
project's `memory` and its `backlog` is a fact about `(project_id, subject)`: a
project created in Kontor is native and writable immediately, while a project
whose facts came from AgentsRoom stays read-only for that subject until its
import and read-back attestation succeed. After attestation, AgentsRoom is
legacy read-only for that subject. Both states can be true of one realm at once.

### Does Kontor store agent transcripts or credentials?

No transcript/token copy is part of Kontor domain state. Session content stays
runtime-owned and is brokered through the daemon. Account records contain
non-secret metadata and credential references, never secret values.

### Is Kontor a cloud service or multi-user server?

No. One daemon owns one local realm and binds loopback only. Remote access, realm
federation, multi-host workers and native mobile pairing require separate
security and identity decisions and are unbuilt.

### Who decides an epic is finished?

Not the seats that did the work, and not a green task graph. The epic's pinned
Completion Profile runs integration, then an independent read-only committee whose
members review frozen evidence without seeing each other's findings, then bounded
remediation if that verdict is non-compliant, then six closeout receipts. A
non-compliant verdict is not waived, and a committee cannot waive it for itself.

### When does Kontor ask a human?

As a last safe step. It first performs deterministic inspection and any
policy-permitted automatic recovery, then uses a bounded Advisor and, for
cross-cutting ambiguity, a bounded Committee, followed only by bounded
evidence-backed remediation. If those paths cannot establish a safe authorized
way forward, Kontor raises one prepared decision brief with the evidence, tried
options, exact decision and consequence of no action. Decisions inherently
reserved to human authority go directly to that brief; consultation cannot
invent authority.

### How do I add another runtime?

Implement the shared `RuntimeAdapter`, declare only capabilities the runtime
can prove, add recorded contract fixtures, and pass the common consistency and
mutation tests. See [Architecture](ARCHITECTURE.md#runtime-extension-contract).

### Where is the detailed design?

The concise repository contract is [Architecture](ARCHITECTURE.md), which is
readable without the parent checkout. The governing principles, the full
implementation baseline and the active plans live in the parent `asma-modules`
documentation; links are collected at the end of `ARCHITECTURE.md`.
