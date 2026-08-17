# KON-OP-04 QA

Date: 2026-08-17
Status: passed
Scope: ASMA-7873 / KON-OP-04 at `c87cb93`

## Result

The committed implementation passes the workspace QA gate:

- `cargo test --manifest-path Cargo.toml --workspace` — 110 suites, 1394 passed, 0 failed, 8 ignored.
- `cargo clippy --manifest-path Cargo.toml --workspace --all-targets -- -D warnings` — passed.
- `cargo fmt --manifest-path Cargo.toml --all -- --check` — passed.

The first workspace run was blocked only by this execution sandbox refusing a
loopback bind in `memory_parity`. The same command completed successfully when
given the required local-loopback permission; `memory_parity` passed.

## Surface coverage

- Core Team: read, preview, apply, mandatory LSA/TPM normalization and explicit
  epic-seat materialization.
- Quick sessions: derived eligible roles, ensure/replay, invalid-role refusal,
  and stable node/seat reconciliation.
- Promotion: preview/apply, one frozen epic/ECP roster, on-demand-seat absence,
  repeated apply, and explicit roster upgrade.
- Recovery: `operational_promotion` proves promotion and roster authorize or
  roll back together; loopback tests prove an authorized promotion and a
  persisted Quick-session row both resume their missing effects.

## Verdict

`qa-gate`: passed. The remediation's durable promotion/roster atomicity and
Quick-session id reconciliation are covered through both store-level and public
API tests.
