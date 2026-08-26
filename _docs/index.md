# Kontor — Documentation Index

> Canonical implementation-documentation root for the Kontor Rust control plane. Last updated: 2026-08-26.

## Overview

This documentation covers implementation-local knowledge for the `asma-rs-kontor` workspace. Platform-wide Kontor product principles, architecture, decisions and implementation plans remain in the [AI orchestration documentation](../../../_docs/ai-orchestration/index.md); this router contains audits, contracts and other documents whose scope is the Kontor repository implementation itself.

**Start here for *why*:** [Kontor governing principles](../../../_docs/ai-orchestration/architecture/2026-08-26-11-30-architecture-kontor-governing-principles.md) — Autonomy and Delivery Quality, and the fourteen principles every rule in this repository serves. The repository-local restatement, readable without the parent checkout, is [`ARCHITECTURE.md`](../ARCHITECTURE.md).

## Documents by Category

### Audits (`audits/`)

| Document | Summary | Status | When to Load |
|----------|---------|--------|-------------|
| `2026-08-20-18-53-audit-kontor-rust-workspace-code-quality.md` | In-depth snapshot audit of the 240k-line Kontor Rust workspace: source/test composition, file and function size, DRY/YAGNI/SOLID, MCP-tool legitimacy, safety, dependencies, CI gaps, and prioritized remediation. Its 127-tool / 16-worker counts are explicitly historical; current inventory is routed to the repository contract. | 🟤 Point-in-time snapshot | Assessing the audited commit's maintainability or its original findings; use current source for today's counts |

### Contracts (root)

| Document | Summary | Status | When to Load |
|----------|---------|--------|-------------|
| [ASMA-7869 committee remediation gate](https://github.com/Carasent-ASMA/asma-rs-kontor/blob/master/_docs/ASMA-7869-committee-remediation-gate.md) | Architecture boundary for non-destructive Committee failure remediation and clean re-review: how a failed decision is ingested as durable evidence, why advancing the same run to an internal second round poisons it, how a legacy advanced run is reconstructed without rewriting it, and what independently authorized remediation must produce before a separate clean re-review may be consumed. Explicitly not a waiver, a verdict-fabrication path, an immutable-row repair procedure, or permission to reuse a poisoned run. | 🟢 Boundary in force | Changing Committee round handling, remediation authority or completion re-review; recovering a run that advanced prematurely; reviewing anything that touches immutable findings |

## Repository-level documents outside this router

| Document | Scope |
|----------|-------|
| [`../README.md`](../README.md) | What Kontor is and is not, what is built and what is deliberately not, build and run |
| [`../ARCHITECTURE.md`](../ARCHITECTURE.md) | Principles, authority boundaries, consistency model, completion machinery, tool registry, security and extension rules |
| [`../docs/CONFIGURATION.md`](../docs/CONFIGURATION.md) | Where configuration lives, seat supervision, seat MCP surface and serve profiles |
| [`../RECOVERY.md`](../RECOVERY.md) | Backup, snapshot, export, restore, import and credential rotation runbook |
| [`../SECURITY.md`](../SECURITY.md) | Reporting and supported-version policy |
| [`../docs/QUOTA-FALLBACK-PLAN.md`](../docs/QUOTA-FALLBACK-PLAN.md) | Provider quota fallback behaviour |
| `../docs/evidence/KON-*/` | Per-ticket delivery evidence: release notes, QA, review, mutation and closeout records |

## Folder Legend

| Folder | Contents |
|--------|----------|
| `audits/` | Point-in-time code-quality, completeness, security, and compliance audits of the Kontor implementation |

## Related authoritative documentation

- [AI Orchestration documentation index](../../../_docs/ai-orchestration/index.md) — product principles, architecture, plans, operational reports and historical decisions governing Kontor.
- [Kontor governing principles](../../../_docs/ai-orchestration/architecture/2026-08-26-11-30-architecture-kontor-governing-principles.md) — Autonomy and Delivery Quality; read before proposing any Kontor change.
- [Kontor control-plane architecture](../../../_docs/ai-orchestration/architecture/2026-08-08-20-12-architecture-asma-kontor-control-plane.md) — the authoritative platform-wide *how*.
- [`ARCHITECTURE.md`](../ARCHITECTURE.md) — repository-local architecture overview.
- [`README.md`](../README.md) — build, usage, and current repository overview.
