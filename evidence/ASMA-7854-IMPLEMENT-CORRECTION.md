# ASMA-7854 implementation correction handoff

Date: 2026-08-14

Seat: Implement · KON-25 (reused native seat)

Starting submodule commit: `babe668ffc41f1c8b90f9b1ed001e50d99fbde69`

Starting outer commit: `741e7f8e9c781fb80f274c12b19717194c6f0082`

Implementation checkpoint: `bacc476ea5308358c231020174652db24828a1b5`

Implementation tree: `8e158aca5bdb1bb19e5cc8513bd324bd1bf50186`

## Audit findings

- F1 resolved: production constructs `TeamsView` with the kontord client
  (`apps/console/src/shell/App.tsx:49`); the view reads the Realm-bound catalog and
  Teams projection together (`apps/console/src/views/TeamsView.tsx:89-99`). The
  fixture is reachable only through injected tests or the explicitly labelled
  offline-preview action (`apps/console/src/views/TeamsView.tsx:78-84,111-121`).
- `/v1/catalog` and `/v1/teams` plus save/publish routes are registered in
  `crates/kontor-api/src/lib.rs:211-217`; observer/operator enforcement is in
  `crates/kontor-api/src/applications.rs:2015-2074`.
- Draft save and immutable next-revision publish are durable store commands in
  `crates/kontor-store/src/teams.rs:52-117`; the database immutability guards are
  in `crates/kontor-store/migrations/0021_teams_editor.sql:1-39`.
- Daemon read/write projections, including server-resolved policy at the Realm
  cursor, are in `crates/kontor-daemon/src/applications.rs:1987-2071,2261-2348`.
- Thin MCP ToolSpecs (and therefore the existing mechanically generated CLI
  surface) are in `crates/kontor-mcp/src/registry.rs:449-519`; API/MCP authority
  parity is asserted in `tests/contract/mcp_parity.rs:541-544`.
- The console preview consumes `resolved_policy` from the server projection at
  the shared Realm/revision/cursor (`apps/console/src/views/TeamsView.tsx:89-99,`
  `198-216,1006-1015`).
- The OpenAPI contract and `apps/console/src/api/schema.d.ts` were regenerated;
  `pnpm verify:api` reports an empty diff.
- F2 resolved: the executable-contract comment now names the live
  `GET /v1/catalog` trust boundary (`apps/console/src/state/teams.ts:1133-1140`).
  The remaining fixture wording identifies tests/explicit offline preview only
  (`apps/console/src/state/teams.ts:1230-1232`). No contradictory `not wired` or
  `stand-in` wording remains in the owned production tree.

## Three binding gates

1. Telemetry/lead: need bands retain telemetry provenance and promotion state
   (`apps/console/src/state/teams.ts:1506-1564`); the ambiguous telemetry `Lead`
   is explicitly mapped to `qa`, not the unqualified team `lead`
   (`apps/console/src/state/teams.ts:1506-1517`).
2. Catalog trust boundary: the live response is passed through
   `validateCatalog` before an editor is rendered
   (`apps/console/src/views/TeamsView.tsx:89-99,127-139`). The live mutant-killing
   regression is `apps/console/src/views/TeamsView.test.tsx:217-236`.
3. F5 count: blocking findings are deduplicated by `(code, slot)` using the key
   at `apps/console/src/views/TeamsView.tsx:965-977`. The live projection mutant
   regression is `apps/console/src/views/TeamsView.test.tsx:612-635`.

The live clamp regression is `apps/console/src/views/TeamsView.test.tsx:637-655`.
Actual mutations were applied and restored: removing live `validateCatalog`
failed its alert assertion; replacing the `(code,slot)` key with unique indices
failed with `6 blocking` instead of `3`. Baseline was rerun after restoration.

## Browser evidence

- `pnpm test:e2e`: 2 passed (desktop 1440x1000, phone 390x844).
- Test: `apps/console/e2e/teams.spec.ts:40-50`.
- Screenshots: `evidence/ASMA-7854-PLAYWRIGHT-DESKTOP.png` and
  `evidence/ASMA-7854-PLAYWRIGHT-PHONE.png`.

## Gates

- Console `pnpm test`: 14 files, 278 passed.
- Console `pnpm typecheck`: exit 0.
- Console `pnpm build`: exit 0, 52 modules transformed.
- Console `pnpm verify:api`: exit 0, generated schema byte-matched.
- `cargo test -p kontor-teams`: 43 passed.
- Core `a_model_chain_is_closed_and_bounded`: 1 passed.
- Live daemon Teams/catalog loopback: 1 passed.
- MCP parity: 11 passed.
- `cargo test -p kontor-cli`: 14 passed across unit/version tests.
- `cargo fmt --all -- --check`: exit 0.
- Relevant five-crate `cargo clippy --all-targets -- -D warnings`: exit 0.
- `Cargo.lock` restored byte-for-byte: SHA-1
  `7515bc48689cd46551278e2ca895763e3ae4ef9d`, Git blob
  `03c9e37793cf1de5b1e385f47991784f4c725b35`, identical to `HEAD:Cargo.lock`.

## Serialized-integration migration collision

This standalone ticket branch is coherent from base `3cf8221` with
`0021_teams_editor.sql`. That filename is **not integration-final**. KON-23 owns
committed sibling migration `0021_native_memory.sql` at `53a2eb9` (with its
correction commit forthcoming). No gap/no-op was fabricated and no sibling work
was merged here.

Serialized integration MUST:

1. Merge KON-23 first.
2. Rename this ticket's migration to `0022_teams_editor.sql`.
3. Update the migration registry and schema-version expectations.
4. Rerun the full gates before creating the integrated checkpoint.

No push, PR, QA, Audit, or integration action was started by this seat.
