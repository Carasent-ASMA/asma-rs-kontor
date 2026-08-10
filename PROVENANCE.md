# Provenance

This repository is created by Carasent ASMA for the Kontor control plane MVP
(plan `2026-08-08-20-12-plan-asma-kontor-mvp-control-plane.md`, Jira epic
`ASMA-7744`). The scaffold (KON-MVP-02) was authored from that plan and the
Kontor architecture (`2026-08-08-20-12-architecture-asma-kontor-control-plane.md`).

## What lives here

- `crates/*`, `apps/*`, `tests/*`: Kontor source and test code, dual-licensed
  `MIT OR Apache-2.0`, owned by Carasent ASMA.
- `Cargo.lock`, `pnpm-lock.yaml`: generated dependency locks, committed as the
  reproducible baseline (CON-007).
- `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`, `THIRD_PARTY_NOTICES.md`:
  licensing boundary for this repository's own code.

## What is deliberately NOT here

- Paseo, Agent Orchestrator and Codex runtimes: separately installed by the
  operator; Kontor only talks to their supported interfaces.
- The `asma` CLI (`_tools/asma-cli`): a separate repository; Kontor delegates
  fleet/Jira operations to its supported commands.
- Any credentials, tokens, keychain data or provider configuration.

## Release prerequisites (carried from the architecture, §19)

Before any public release: employer intellectual-property review, product-name
clearance, and a final review of `NOTICE` and `THIRD_PARTY_NOTICES.md` against
the actual locked dependency set (`cargo deny` + `pnpm audit` output).
