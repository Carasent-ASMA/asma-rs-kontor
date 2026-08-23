# KON-OP-20 closeout

Date: 2026-08-23
Jira: ASMA-7968
Status: implementation and local verification complete; merge and live verification pending

## Delivered

- Project topology selection has typed preview and apply operations with
  revision and preview-hash checks.
- Selection is stored through migration `0056_project_topology_selection.sql`
  and exposed through the API, MCP registry and generated console contract.
- The runtime catalog includes OpenCode and DeepSeek choices.
- Loopback coverage proves the project selection flow and the existing seat
  message acknowledgement/reconciliation path.
- The live runtime catalog currently reports no unavailable providers.

## Verification

The workspace formatting, Clippy, Rust tests, console typecheck/tests/build,
`cargo audit`, and `cargo deny check` gates were run against this combined
closeout branch. Merge and live readback are intentionally not claimed here.
