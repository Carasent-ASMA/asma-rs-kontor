# KON-MVP-20 committed-archive gates

Anchor: outer `f9e341440b140ec4b94fbfeadfe5f52fd8e0ea89`, gitlink and
submodule `5cc0e223e8f297f551bb521c580508395620d432`. Toolchain: Rust/Cargo
`1.97.1`, Node `24.13.1`, pnpm `11.6.0`, cargo-audit `0.22.2`, cargo-deny
`0.20.2`, Python `3.14.2`.

| Gate, run from disposable archive | Result |
|---|---|
| `python3 scripts/verify-tree.py --mode archive` | PASS, exit 0; log SHA-256 `06eb3157fb0d50319e3065006d8663d6a225ce13cef42f5aa65e35b44b17f3ed` |
| archive `cargo generate-lockfile` plus byte compare | PASS; before, regenerated, and committed lock all `2e89a646b8a4340951a96f4a655adcfafa82922c9943751657929894624f8179`; log `c38ac31fc11b043cac9d9ec6d2f68880d8e5a36f2f6afa120e46cdcbf1d55d59` |
| `cargo fmt --all -- --check` | PASS (inside archive verifier) |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `cargo test --workspace --locked` | PASS; 1,238 successful Rust test/doctest cases, 0 failed |
| `cargo audit` | PASS; only reported expected informational unmaintained warnings |
| `cargo deny check` | PASS: advisories, bans, licenses, and sources all OK; duplicate-version notices are warnings |
| `pnpm install --frozen-lockfile` | PASS |
| `pnpm -r typecheck` | PASS |
| `pnpm -r test` | PASS; console 14 files / 278 tests |
| `pnpm audit --prod` | PASS; no known vulnerability |
| `pnpm --filter kontor-console verify:api` | PASS; log SHA-256 `c7431a697a05b5343545d2a0b67381bb3e39a10d4953576599af3f594d6796eb` |
| `pnpm -r build` | PASS; production Vite bundle; log `a206530db6009139e81e8c6d6ae209a08c04119d1d21f7e47a60e10aed6a2b6c` |
| `pnpm --filter kontor-console test:e2e` | PASS; Playwright desktop + phone 2/2; log `6156b94b7d0e5e6e45553e4c232c774a8105923e06e46aa519d1c25bfa544031` |
| targeted `cargo test -p kontor-daemon --test mcp_journey --locked` | PASS, 2/2; log `bc4a1d6c95c78edf5b53cb79d271f22fd34cd3d6f319af816be797e6861fe554` |

Playwright screenshots were produced in the disposable archive only:
desktop SHA-256 `cee018dbb118a6b6914e3f8b53065f831d7d24b3ef9990b4c3a77bc0dd6f6132`,
phone `24bb7690fb106e77168e1d496a09d434e865ff42bb29f35b5fbcbe254be617a1`.
Generated OpenAPI and
`schema.d.ts` verification was byte-clean. No gate read the dirty validation
checkout, and no generated artifact or `Cargo.lock` is part of this evidence
commit.
