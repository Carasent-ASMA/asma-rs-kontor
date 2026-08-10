# Third-party notices

Kontor builds on open-source dependencies. This file defines the boundary and
the audit mechanism; it is regenerated and reviewed as part of the release
process.

## Rust dependencies

The complete, exact dependency tree is the committed `Cargo.lock`. The
automated license and advisory gates are:

- `cargo audit` — known-vulnerability advisories.
- `cargo deny check` — license allowlist, duplicate-version policy and
  yanked-crate policy, configured in `deny.toml`.

A human-readable license report can be produced with:

```sh
cargo license
```

(or `cargo deny list`). The final release must attach the generated license
manifest for the exact locked versions.

## JavaScript dependencies

The exact tree is `pnpm-lock.yaml`. Production advisories are checked with
`pnpm audit --prod`. The console's runtime dependencies are React,
React DOM and the Tauri JS API/Stronghold plugin (used from KON-MVP-17
onwards); development tooling (Vite, TypeScript, Vitest, Testing Library,
Playwright, openapi-typescript) is not distributed.

## Separately installed runtimes

Kontor orchestrates Paseo, Agent Orchestrator and Codex sessions and delegates
fleet/Jira operations to the `asma` CLI. These are separate installations with
their own licenses and are never distributed from this repository.

## License texts

The license texts for the two licenses under which Kontor itself is published
are in `LICENSE-MIT` and `LICENSE-APACHE`.
