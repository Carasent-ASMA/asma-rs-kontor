# Changelog

All notable changes to Kontor are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and releases use [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
