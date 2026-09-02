# Changelog

All notable changes to Kontor are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and releases use [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Added immutable project-scoped epic backlog codes, deterministic
  collision-safe allocation, manual overrides and schema-v72 legacy evidence
  migration.
- Added the typed `ITEM_CODE` native-name token and Operational topology v4 for
  `ESW · KOP-8001`, `ECP · KOP-8001` and `TSW · KOP-7869` rendering from
  confirmed Jira identities.
- Added durable, exact-seat Committee permission inspection and response through
  the HTTP, CLI and MCP surfaces, including a schema-v75 effect ledger and the
  narrow leadership serve-profile capabilities needed by persistent LSA/TPM
  seats.

### Changed

- Extended epic preview/apply/readback and the generated OpenAPI/TypeScript
  contracts with the Kontor-owned epic backlog namespace while preserving full
  Jira issue keys as the binding authority.
- Advanced the immutable Operational topology opt-in to revision 4; revision 1
  remains unchanged.

### Fixed

- Limited Team Definition migration previews and exact persistence censuses to
  active topology nodes and seats, leaving retired or archived native history
  untouched instead of requiring an unavailable historical workspace retitle.
- Made Jira materialization retries safely adopt an exact existing task binding
  after a lost connector response without creating a duplicate link, and
  prevented already-confirmed materialization items from being rebound.
- Kept pre-schema-v72 epics readable and operable while they await an explicit
  backlog-code assignment; topology v4 still refuses to materialize without it.
- Required standalone native-container retitles to preserve the exact native
  identity and read back the requested title before recording a success receipt.
- Recover failed Jira create materialization in place against the original
  schema-v74 batch and immutable marker ledger. Recovery requires an exact Jira
  project/type/parent/summary/description/marker match, maps by ordinal instead
  of response position, and cannot rebind an already-confirmed epic.
- Compose seat MCP configuration atomically even when an ECP is not a Git
  checkout, leaving no partial `.mcp.json` or harness directory on preflight
  refusal.

## [0.2.1] - 2026-08-30

### Fixed

- Recovered stale or missing hosted Core Team natives on their governed model
  route while preserving the stable logical seat and fenced native history.
- Extended durable global Committee rounds through the scheduler's full
  positive round domain so a needs-human recovery can enter round three.
- Persisted exact-occupancy Completion-wake delivery, canonical timeline
  acknowledgement, restart reconciliation, and newest-projection handover to
  a replacement TPM without duplicate native effects.
- Refreshed the console's development-only OpenAPI dependency chain to consume
  the patched `js-yaml` release.

## [0.2.0] - 2026-08-26

### Added

- Durable, local-first orchestration state for projects, tasks, teams, seats,
  gates, evidence, approved memory, runtime bindings and external projections.
- Deterministic admission, guardrails, completion profiles, independent review,
  bounded remediation and receipt-backed recovery.
- A production Paseo runtime adapter, native Jira and provider-quota connectors,
  a generated CLI/MCP capability surface, and responsive operator console.
- Governing documentation for Kontor's two product values: Autonomy and Delivery
  Quality.

### Changed

- Synchronized architecture, configuration, recovery and quota-routing
  documentation with schema v66 and the current 146-tool registry.
- Clarified that admission is default-allow and authorization only narrows work.
- Distinguished shipped account-before-rung resolution from the unbuilt
  launch-time `Wait` / `NeedsHuman` actuation and mid-run succession paths.
- Marked calendar exposure, post-delivery profile packs, automatic stale-evidence
  rejection and the supervision engine as unfinished capabilities.

[Unreleased]: https://github.com/Carasent-ASMA/asma-rs-kontor/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/Carasent-ASMA/asma-rs-kontor/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/Carasent-ASMA/asma-rs-kontor/releases/tag/v0.2.0
