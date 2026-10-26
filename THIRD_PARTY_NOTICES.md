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
Playwright, openapi-typescript) is not distributed. The Paseo plugin
(`apps/paseo-plugin`) has no distributed runtime dependencies: Paseo provides
its plugin SDK, and the `@getpaseo/*`, TypeScript and Vitest packages are
development and test tooling.

## Separately installed runtimes

Kontor orchestrates Paseo, Agent Orchestrator and Codex sessions; the `asma`
CLI calls Kontor's publication endpoints. These are separate installations with
their own licenses and are never distributed from this repository.

## License texts

The license texts for the two licenses under which Kontor itself is published
are in `LICENSE-MIT` and `LICENSE-APACHE`.

## Paseo acknowledgement patch

`docs/upstream/paseo-v0.12.0-mark-seen.patch` contains modified source and
context from [Paseo v0.12.0](https://github.com/getpaseo/paseo/tree/cafe4b01c8522ff39560241c5deb01427dc8dc5b),
Copyright (c) 2025-present Mohamed Boudra. That revision licenses this source
under Apache-2.0; see `LICENSE-APACHE`. The patch is a source candidate for
metadata-only read acknowledgement, not a bundled or installed Paseo runtime.
