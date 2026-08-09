# Kontor control plane

Kontor is a local-first control plane that plans, launches, supervises and
audits long-lived project work across existing agent runtimes. This repository
is the public-ready source for the MVP defined by the ASMA Kontor MVP plan
(`asma-modules/_docs/ai-orchestration/plans/2026-08-08-20-12-plan-asma-kontor-mvp-control-plane.md`).

> **Status: scaffold (KON-MVP-02).** The workspace, locked dependency set,
> licenses, CI gates and committed-tree verification are established. The
> crates are documented placeholders owned by their tickets; no domain logic
> exists yet.

## Repository layout

```text
crates/
  kontor-core/                domain identifiers, states and commands (KON-MVP-03)
  kontor-store/               SQLite repositories, migrations, event append/replay (KON-MVP-03)
  kontor-runtime/             adapter contract, capabilities and normalized events (KON-MVP-05)
  kontor-runtime-paseo/       Paseo CLI/API integration (KON-MVP-11)
  kontor-runtime-ao/          Agent Orchestrator REST/SSE/WS integration (KON-MVP-12)
  kontor-runtime-codex/       narrow direct Codex account-isolation fallback (KON-MVP-13)
  kontor-context/             Context Pack resolution and portable handoffs (KON-MVP-06)
  kontor-accounts/            non-secret profiles, credential references and routing (KON-MVP-07)
  kontor-teams/               versioned templates and run snapshots (KON-MVP-08)
  kontor-profiles/            composable work profiles, teams and persona scenarios (KON-MVP-08)
  kontor-scheduler/           ready-set, leases, budgets and explanations (KON-MVP-09)
  kontor-calendar/            work windows, holiday imports, drain state and overrides (KON-MVP-21)
  kontor-policy/              guardrail evaluation and evidence (KON-MVP-10)
  kontor-integrations-asma/   asma fleet / asma jira sync subprocess boundaries (KON-MVP-14)
  kontor-api/                 versioned HTTP JSON and SSE contract (KON-MVP-15)
  kontor-daemon/              single-instance process and composition root (KON-MVP-15)
  kontor-cli/                 the `kontor` binary (KON-MVP-16)
  kontor-mcp/                 stdio MCP server (KON-MVP-16)
  kontor-intake/              source-event intake, deduplication and triggers (KON-MVP-22)
apps/
  console/                    React/TypeScript operator console (KON-MVP-17)
  desktop/                    Tauri 2 desktop shell for the console (KON-MVP-17)
tests/
  contract/                   shared contract test crate (workspace member)
  e2e/                        end-to-end test crate (workspace member)
  fixtures/                   test data only (not a crate)
```

## Prerequisites

- Rust toolchain pinned in `rust-toolchain.toml` (`1.97.1`); install with
  `rustup` (rustfmt and clippy components are pinned there).
- Node.js `>=24` and pnpm `>=11` for `apps/console` and `apps/desktop`.
- Tauri 2 desktop builds additionally require the official platform
  prerequisites (Linux: webkit2gtk-4.1 and friends; see `.github/workflows/ci.yml`).

## Local gates

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

`scripts/verify-tree.py` reruns every gate from a clean export of the staged
tree (`--mode staged`, no `.git`) or of a committed `HEAD` (`--mode archive`),
and byte-compares the regenerated `Cargo.lock` against the committed one. After
an authorized commit, run:

```sh
python3 scripts/verify-tree.py --mode archive
```

## Dependency policy (CON-007)

The root `Cargo.toml` owns the workspace member list and the exact shared
dependency pins. Later tickets must not edit them without re-planning; new
dependencies are added to `[workspace.dependencies]` once and referenced by
members with `workspace = true`. The generated `Cargo.lock` is committed.

## License and provenance

Kontor source is dual-licensed `MIT OR Apache-2.0` (SPDX) — see `LICENSE-MIT`
and `LICENSE-APACHE`, plus `NOTICE`, `PROVENANCE.md` and
`THIRD_PARTY_NOTICES.md`. Kontor is a control plane over separately installed
runtimes (Paseo, Agent Orchestrator, Codex, `asma` CLI); those remain separate
installations and are not distributed here.

Public release remains gated on employer-IP review, name clearance and a final
dependency NOTICE review (Kontor architecture, §19).
