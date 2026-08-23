# KON-OP-07 native Jira closeout

Date: 2026-08-23
Jira: ASMA-7876, completed by successors ASMA-8014 and ASMA-8015
Status: implementation and local verification complete; merge and live verification pending

## Delivered

- Jira transport and policy now run in the native `kontor-jira` crate.
- `kontor-daemon` no longer executes `asma jira sync` or depends on
  `kontor-integrations-asma`.
- Jira configuration is project-scoped, contains only a credential alias, and
  resolves the secret from the system keychain for each call.
- Requests reject redirects, off-origin responses and oversized bodies.
- Epic and issue materialization persists intent before effects, uses stable
  markers to recover ambiguous creates, and confirms results from readback.
- Memory and backlog cutover use the project/subject authority ledger. Backlog
  import is now available before the one-way authority switch.
- Legacy ASMA CLI Jira mutation paths fail closed; read-only and dry-run paths
  remain available.

## Verification

- `cargo check --workspace --all-targets`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --all-targets` (final run pending at document creation)
- `cargo test -p kontor-jira`
- `cargo test -p kontor-store --lib`
- `pnpm -r typecheck`
- `pnpm -r test`
- console production build
- `cargo audit` (only explicitly allowed advisories)
- `cargo deny check`

Three targeted mutants were killed and reverted: removal of the cutover
readback guard, duplicate Jira marker adoption, and bypass of the ASMA CLI
machine-write refusal.

This document does not claim merge or deployment. Those receipts must be added
to Jira only after the merged revision is running and read back from the live
Kontor daemon.
