# KON-MVP-20 EVD-027 / EVD-028 / EVD-029 inputs

All code/test references are from archive commit
`5cc0e223e8f297f551bb521c580508395620d432`.

## EVD-027 — memory ledger, Context Pack, and cutover

- Native versioned memory and immutable proposal/approval/current semantics:
  `crates/kontor-store/src/memory.rs:128-352`.
- Frozen Context Pack binding persists ordered revision ids and result hash once:
  `crates/kontor-store/src/memory.rs:386,633`; ML06 and ML07 are killed.
- FTS rebuilds from approved current, non-tombstoned revisions only:
  `crates/kontor-store/src/memory.rs:349-352`; ML05, ML08, ML09 are killed.
- Hashed import, receipt, freeze/switch, and post-cutover refusal:
  `crates/kontor-store/src/memory.rs:494-565`; ML10-ML12 are killed.
- Backup restore invokes the memory FTS rebuild at
  `crates/kontor-store/src/backup/restore.rs:112-135`; backup/export suite passes.
- Child evidence hashes: `docs/evidence/KON-MVP-23/QA-MEMORY-MUTANTS.md`
  `53b71cb32e0b0088dfbb13404bc929d625cc77dc02abf6f16df4ef7c25a7eb28`,
  `AUDIT-MEMORY-MUTANTS.md`
  `69ed3767ea840d17bad2747a81a9f5108cad6347fcd57a74bc80f5937a53bade`.

Result input: **13/13 memory mutants killed**, export/restore and archive gates
green, no cross-project read and no post-cutover dual writer.

## EVD-028 — compaction-policy continuity

The governing outer input is
`_docs/ai-orchestration/reports/2026-08-13-10-08-report-seat-context-transition-evidence.md`.
Its SHA-256 is
`846431761c0b1ac8e0b375c7a58646bdc5505b99050667e9492f0fe8517f06b7`.
The committed runtime/session model retains the same native seat and strict
timeline position across reuse; the real-Paseo bundle additionally proves both
bound seats settled and restart retained native identity. L13 and the client
session/control suites prove epoch/sequence discontinuity forces refetch rather
than being hidden by compaction or replay.

Result input: policy continuity is supported by the committed contract plus the
real restart record; it does not claim character-level terminal persistence.

## EVD-029 — Teams live catalog and surface parity

- Public kontord catalogue/API parity is exercised in
  `crates/kontor-daemon/tests/loopback_api.rs` and `tests/contract/mcp_parity.rs`.
- Teams editor behavior is pinned by 278 console tests and the production build.
- `apps/console/e2e/teams.spec.ts:43` passes in desktop and phone Playwright
  projects; screenshots hash to
  `cee018dbb118a6b6914e3f8b53065f831d7d24b3ef9990b4c3a77bc0dd6f6132`
  and `f7eea548d968430689eef1713d108ed21a62a5011ce180498e0e60f0b9273442`.
- OpenAPI-to-client verification is clean (`verify:api` exit 0).

Result input: API/CLI/MCP catalogue parity and responsive editor behavior are
green on the archive. The Playwright test controls the catalog boundary; it is
not presented as a second real network/daemon run. The separate loopback suite
is the committed live-kontord boundary proof.
