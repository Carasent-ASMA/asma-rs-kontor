---
title: Kontor memory runtime parity
category: history
status: completed
date_created: 2026-08-23
date_updated: 2026-08-23
owners: [Kontor]
tags: [memory, context-pack, runtime, ASMA-7821]
---

# Kontor Memory Runtime Parity

This ASMA-7821 recovery now delivers every task's approved, current,
untombstoned project-memory revisions to initial, added, and replacement worker
seats through the existing canonical Context Pack and runtime prompt.

## Implementation receipt

- The store freezes the canonical pack and ordered memory binding atomically in the existing `context_packs` and `memory_context_bindings` tables.
- Retries and snapshots read the original frozen bytes, identifiers, hash, and revision provenance instead of resolving live memory again.
- Preview, snapshot, and launch share one context builder. Launch prompts delimit memory as reference data that cannot grant tools, permissions, approvals, or authority.
- Runtime adapters and public OpenAPI/MCP contracts did not change; the existing bounded prompt is the shared delivery channel.
- Focused mutation checks killed orphan-pack, omitted-delivery, and retry-recompute defects.
- `cargo fmt --all -- --check`, workspace Clippy with warnings denied, and `cargo test --workspace --all-targets` passed. A duplicate concurrent workspace run caused one expected SQLite-busy result; its isolated rerun passed.

Recovered implementation checkpoint: `262d409` (originally committed under the
unrelated ASMA-8014 authority-ledger ticket).

## Deliberate limits

The implementation uses deterministic inclusion of all approved current project memory and fails closed at the existing 65,536-character prompt limit. Add a versioned relevance selector only after measured project corpora approach that ceiling. Vector storage, bulk AgentsRoom-note migration, and memory UI work remain out of scope.
