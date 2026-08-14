# ASMA-7854 / KON-MVP-25 — IMPLEMENT handoff

Date: 2026-08-14
TSW: `/Users/igor/.paseo/worktrees/0vl4ss0m/feat-asma-7854-kon-mvp-25`
Outer branch: `feat/ASMA-7854-kon-mvp-25`
Submodule base before checkpoint: `3cf8221efb0b6497b1069b526b6960d5072f1127` (detached submodule checkout)
Prototype source: `.worktrees/proto-kontor-teams`, seven console paths ported and remediated.

## Blocking acceptance gates — evidence for QA

1. Telemetry-derived bands and `lead` resolution
   - `apps/console/src/state/teams.ts:1500` explicitly maps unqualified `lead` to `architect` (Lead Architect) and `manualTestLead` to `qa`; they are not conflated.
   - `apps/console/src/state/teams.ts:1505-1519` records the exact AgentsRoom stats file, derivation (median non-zero per-agent/day token movement per prompt), observation range/date, and keeps the result `fixture/needs-verification` until a review signs it.
   - `apps/console/src/state/teams.test.ts:289-295` proves the mapping, citation, unpromoted state, and that telemetry can disagree with the seeded class (the circularity mutant is dead).

2. `validateCatalog` at the `/v1/catalog` trust boundary
   - `apps/console/src/views/TeamsView.tsx:69-84` calls `validateCatalog` before any catalog-backed editor renders; blocking provenance defects render a refusal and admit no controls.
   - `apps/console/src/state/teams.ts:150-174` documents the real enforcement path; the old “not wired” placeholder is removed.
   - `apps/console/src/views/TeamsView.test.tsx:196-210` injects an unsigned promoted catalog, asserts the `/v1/catalog` refusal, and proves the template list is absent.

3. F5 blocking-count deduplication
   - `apps/console/src/views/TeamsView.tsx:869-881` attaches the slot id and counts a `Set` keyed by `(issue.code, issue.slot)`.
   - `apps/console/src/views/TeamsView.test.tsx:527-548` makes the same three provenance defects arrive through both validation paths and proves the badge says `3 blocking`, not `6`.

These three items are binding. QA/Audit/integration must not proceed if any cited check is removed or fails.

## Acceptance-criterion implementation map

- AC-1: Teams rail entry and responsive view — `apps/console/src/shell/NavRail.tsx:20`, `apps/console/src/shell/App.tsx:114-117`, `apps/console/src/console.css:474-716`.
- AC-2: immutable monotonic revisions — `apps/console/src/state/teams.ts:498-515`, editor publication/list at `apps/console/src/views/TeamsView.tsx:133-158`, tests at `apps/console/src/state/teams.test.ts:298-308` and `apps/console/src/views/TeamsView.test.tsx:511-525`.
- AC-3: catalog-constrained provider/model/effort/context and explicit-only handling — `apps/console/src/state/teams.ts:552-812`, controls at `apps/console/src/views/TeamsView.tsx:499-622`, context table at `:684-781`.
- AC-4: cross-provider rung 2, pooled-provider fallback, raw effort and derived verdict — `apps/console/src/state/teams.ts:552-688`, `apps/console/src/views/TeamsView.tsx:349-357`.
- AC-5: resolved preview contains class/source/effective/enforcement/capability/latest receipt — `apps/console/src/views/TeamsView.tsx:439-444`, test `apps/console/src/views/TeamsView.test.tsx:373-383`.
- AC-6: no client provider probing, credentials or scheduler state; rules are pure catalog consumers — module contract `apps/console/src/state/teams.ts:1-38`; shell retains the existing realm client boundary.
- AC-7: target/effective threshold, charging basis, sourced metered-only dollars, coverage recommendation, and task-minimum precedence — `apps/console/src/state/teams.ts:696-938`, `:991-1100`; UI `apps/console/src/views/TeamsView.tsx:400-497`; precedence test `apps/console/src/state/teams.test.ts:311-320`.
- AC-8: per-cell provenance and gate reference; unreviewed economics remain visibly unpromoted — `apps/console/src/state/teams.ts:42-190`, fixture promotion authority at `:80`, UI badges throughout the chain/class/need editors.

Provider-neutral Rust schema: `EffortLevel`, `ProviderRef`, `ModelRef`, `ModelRung`, and `ModelChainPolicy` are at `crates/kontor-core/src/spec.rs:17-89`; `RoleSlotSpec.model_chain` is at `crates/kontor-teams/src/spec.rs:104-106` and is validated with the template.

## Verification completed before boundary

- Console full suite: 14 files, 270 passed (before the final preview/task-minimum additions).
- Final focused console suite: 2 files, 117 passed.
- Console TypeScript: `pnpm typecheck` exit 0.
- Console production build: exit 0, 52 modules transformed (before the final presentation-only preview line; typecheck rerun after it).
- Rust model-chain check: 1 passed.
- `cargo test -p kontor-teams`: 43 passed.
- `cargo fmt --all -- --check`: clean after formatting.
- `pnpm lint`: unavailable (`Command "lint" not found`), not a failed lint run.
- `Cargo.lock` was restored after Cargo tried to refresh unrelated stale workspace entries; it is not part of this change.

## Boundary note

No QA/Audit/integration/ship action was started in this seat. Keep this same native seat for follow-up; do not replace it merely to obtain a fresh context.
