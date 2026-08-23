# Kontor — Documentation Index

> Canonical implementation-documentation root for the Kontor Rust control plane. Last updated: 2026-08-23.

## Overview

This documentation covers implementation-local knowledge for the `asma-rs-kontor` workspace. Platform-wide Kontor product architecture, decisions, and implementation plans remain in the [AI orchestration documentation](../../../_docs/ai-orchestration/index.md); this router contains audits and other documents whose scope is the Kontor repository implementation itself.

## Documents by Category

### Audits (`audits/`)

| Document | Summary | Status | When to Load |
|----------|---------|--------|-------------|
| `2026-08-20-18-53-audit-kontor-rust-workspace-code-quality.md` | In-depth snapshot audit of the 240k-line Kontor Rust workspace: source/test composition, file and function size, DRY/YAGNI/SOLID, MCP-tool legitimacy, safety, dependencies, CI gaps, and prioritized remediation. | 🟡 In Review | Assessing Kontor Rust maintainability; planning decomposition or quality gates; adding capabilities, broad port methods, or MCP tools |

### History (`history/`)

| Document | Summary | Status | When to Load |
|----------|---------|--------|-------------|
| `2026-08-23-16-16-history-kontor-memory-runtime-parity.md` | ASMA-7821 recovery receipt for frozen approved memory delivery to every worker launch. | ✅ Completed | Reviewing the Kontor memory launch contract, its verification, or its deliberate retrieval limits |

## Folder Legend

| Folder | Contents |
|--------|----------|
| `audits/` | Point-in-time code-quality, completeness, security, and compliance audits of the Kontor implementation |
| `history/` | Concise receipts for completed implementation plans |

## Related authoritative documentation

- [AI Orchestration documentation index](../../../_docs/ai-orchestration/index.md) — product architecture, plans, operational reports, and historical decisions governing Kontor.
- [`ARCHITECTURE.md`](../ARCHITECTURE.md) — repository-local architecture overview.
- [`README.md`](../README.md) — build, usage, and current repository overview.
