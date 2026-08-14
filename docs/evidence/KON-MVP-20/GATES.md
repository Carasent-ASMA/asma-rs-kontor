# KON-MVP-20 corrective committed-source gates

Validated 2026-08-15 from a disposable `git archive` of committed source
anchor `97791ab1aff72d2dfbaeffaa72b2b631705f4356`. The evidence-only successor
changes this directory, not the validated product source. Source archive
SHA-256: `3b910fd1d09bef8fc27430670d2da4dc650b0b9af0fc7116dc4f1b85b4dbfffa`.
Committed and regenerated `Cargo.lock` SHA-256:
`781ae8a2e7b5c437066a3b76c255d7d097dc439e5a66dc2e7b43b2f1c7074e26`.

Toolchain: Rust/Cargo `1.97.1`, Node `24.13.1`, pnpm `11.6.0`, cargo-audit
`0.22.2`, cargo-deny `0.20.2`.

| Gate | Result |
|---|---|
| `python3 scripts/verify-tree.py --mode archive` | PASS, exit 0; includes reproducible lock byte-compare and every Rust/console gate below |
| regenerated `Cargo.lock` byte compare | PASS, byte-identical |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `cargo test --workspace --locked` | PASS, 0 failed |
| `cargo audit` | PASS; only the repository's allowed informational unmaintained warnings |
| `cargo deny check` | PASS: advisories, bans, licenses, and sources |
| `pnpm install --frozen-lockfile` | PASS |
| `pnpm -r typecheck` | PASS |
| `pnpm -r test` | PASS; console 14 files / 278 tests |
| `pnpm audit --prod` | PASS; no known vulnerabilities |
| `pnpm --filter kontor-console verify:api` | PASS; generated API is byte-clean |
| `pnpm -r build` | PASS |
| `pnpm --filter kontor-console test:e2e` | PASS; Playwright desktop + phone 2/2 |
| corrective mutation delta C01-C04 | PASS; 4/4 killed, combined ledger 31/31 |

Playwright screenshots match the committed artifacts: desktop SHA-256
`cee018dbb118a6b6914e3f8b53065f831d7d24b3ef9990b4c3a77bc0dd6f6132`,
phone `f7eea548d968430689eef1713d108ed21a62a5011ce180498e0e60f0b9273442`.
No generated test result was staged.
