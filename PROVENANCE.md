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

## What a Kontor realm produces, and what may leave it

Snapshots (`kontor-<realm>-<instant>.db`) are byte copies of a realm's own
database. They carry that realm's operational data and are **not** a
distributable artifact of this repository; they stay wherever the operator's own
data-handling rules put them.

The redacted export (`KontorExportV1`) is the only document designed to leave a
machine. It is versioned, deterministic and scanned before it is written: no
credential, credential reference, environment mapping, connector payload, Zone C
material, runtime transcript or token delta is in it, and it names in its own
`redaction_summary` what it withheld. See `RECOVERY.md`.

## Release prerequisites (carried from the architecture, §19)

Before any public release: employer intellectual-property review, product-name
clearance, and a final review of `NOTICE` and `THIRD_PARTY_NOTICES.md` against
the actual locked dependency set (`cargo deny` + `pnpm audit` output).
