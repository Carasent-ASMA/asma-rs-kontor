# ASMA-7854 / KON-MVP-25 — QA verdict

```yaml
date: 2026-08-14
outer_commit: 741e7f8e9c781fb80f274c12b19717194c6f0082
submodule_commit: babe668ffc41f1c8b90f9b1ed001e50d99fbde69
design_gate: /Users/igor/kon-mvp-20-scratch/evidence/kontor-teams/RE-GATE-RECORD.md
design_gate_verdict: COMPLIANT
overall: READY-FOR-AUDIT
```

## Verification commands

| Gate | Result |
|---|---|
| `apps/console: pnpm test` | PASS — 14 files, 272 tests |
| `apps/console: pnpm typecheck` | PASS — exit 0 |
| `apps/console: pnpm build` | PASS — 52 modules transformed |
| `cargo test -p kontor-teams` | PASS — 43 passed, 0 failed |
| `cargo test -p kontor-core --test spec_validation a_model_chain_is_closed_and_bounded` | PASS — 1 passed, 44 filtered |
| `cargo fmt --all -- --check` | PASS |

The handoff's 117 focused-test count is superseded by the final frozen-tree full-suite result of
272 passed tests. No lint script is defined in `apps/console/package.json`.

## Acceptance criteria

| AC | Verdict | Evidence |
|---:|---|---|
| 1 | PASS | Teams rail/view is wired in `NavRail.tsx:20` and `App.tsx:114-117`. The responsive Teams surface is covered by `console.css:474-716` and the Teams view tests. |
| 2 | PASS | `publishTeamRevision` computes the next immutable revision at `teams.ts:505-514`; state and view tests cover monotonic versions and earlier-revision immutability (`teams.test.ts:299-308`, `TeamsView.test.tsx:515-525`). |
| 3 | PASS | Provider/model/effort/context controls are catalog-constrained in `teams.ts:552-812` and `TeamsView.tsx:523-647`; tests cover provider narrowing, route effort ladders, unset/no-effort routes, context ceilings and enforcement. |
| 4 | PASS | Rung-2 provider crossing, pooled-provider fallback, raw effort ladders and derived verdict capability are implemented in `teams.ts:564-685`; tests cover cross-provider acceptance, same-provider refusal, pooled repeats, and degraded-lane verdict denial. |
| 5 | PASS | The resolved preview renders class, source, effective threshold, enforcement, capability and latest receipt at `TeamsView.tsx:439-443`; `TeamsView.test.tsx:373-383` asserts all fields. |
| 6 | PASS | `teams.ts:1-8` defines pure catalog rules with no fetch, provider probing, credentials or scheduler state. The shell retains the existing credential/client boundary. |
| 7 | PASS | Context resolution and economics are implemented at `teams.ts:696-938` and composed in `:1003-1103`; the UI is at `TeamsView.tsx:446-508`. Tests cover effective thresholds, clamp/refusal, charging basis, sourced/withheld dollars, smallest covering class and task-minimum precedence (`teams.test.ts:311-320`). |
| 8 | PASS | Per-cell `Provenance`, gate reference and enforcement are implemented at `teams.ts:42-177`; badges/rendering are present throughout the chain, context and need editors. Provenance tests cover unsigned promotions and need-band enforcement (`teams.test.ts:169-285`). |

## Binding acceptance gates

These are mandatory gates. All three pass; QA/Audit/integration may proceed.

1. **Telemetry-derived need bands and explicit lead mapping — PASS.**
   `teams.ts:1500-1519` maps `lead -> architect` and `manualTestLead -> qa`, cites the AgentsRoom
   telemetry file/derivation/date, and leaves each band `fixture/needs-verification` with no review
   reference. `teams.test.ts:289-295` asserts the mapping, citation, unpromoted state, and that
   the telemetry band disagrees with the seeded `deep` class. No unjustified promotion is present.

2. **`validateCatalog` trust-boundary refusal — PASS.**
   `validateCatalog` validates promoted provider/model provenance at `teams.ts:150-177`. `TeamsView.tsx:69-84`
   invokes it before `TeamList` or any catalog-backed editor is constructed and refuses the
   `/v1/catalog` payload on blocking defects. `TeamsView.test.tsx:196-210` injects an unsigned
   promoted catalog, proves the refusal, and proves the template list is absent.

3. **Blocking-count deduplication — PASS.**
   `TeamsView.tsx:869-881` attaches each issue's slot and counts a `Set` keyed by
   `(issue.code, issue.slot)`. `TeamsView.test.tsx:527-548` duplicates the same three provenance
   defects through both validation paths and asserts `3 blocking`, not `6`.

## Mutation/regression evidence

Baseline was green before mutation. Mutants were run one at a time in isolated `/private/tmp`
copies; the frozen source was not mutated.

| Mutant | Result | Killing assertion |
|---|---|---|
| Circular telemetry band changed from `76_000` to circular `512_000` | KILLED | `teams.test.ts:295`: expected recommendation `lean`, received `deep` |
| Removed `reviewSeat` need-band provenance guard | KILLED | `teams.test.ts:253-259`: expected the three blocking provenance codes, received `[]` |
| Removed `validateCatalog` enforcement | KILLED | `teams.test.ts:216`: expected `promotion_without_review_ref`, received none |

No mutant survived and no mutant remains in the checkpoint.

## Tree and lock state

`Cargo.lock` is byte-identical to the committed blob:

```text
worktree: 03c9e37793cf1de5b1e385f47991784f4c725b35
HEAD:     03c9e37793cf1de5b1e385f47991784f4c725b35
```

The outer tree is clean. The submodule has one unrelated pre-existing untracked evidence file,
`evidence/ASMA-7854-AUDIT.md`; no source, test, generated build output or lockfile changes are
present. This QA verdict is the requested new evidence artifact itself.

## Findings

- No acceptance failure found.
- The design gate's two stated admission follow-ups remain honestly unpromoted: formal AC-7
  amendment and review/sign-off of telemetry need bands. They are not silently represented as
  researched facts and do not fail the three binding gates.
- A stale explanatory comment remains in `teams.ts:1120-1123` claiming `validateCatalog` is not
  wired, contradicting the live call in `TeamsView.tsx:70`. This is documentation drift only; the
  executable trust-boundary guard and regression test pass.

## Overall verdict

**READY-FOR-AUDIT**

## Re-QA — corrected live checkpoint (2026-08-14)

```yaml
date: 2026-08-14
outer_commit: 0b5086174ce822be721953ae4318f4467788b410
submodule_commit: 35f4c7e8c8f46f1f4f82875b095ed538e442e378
prior_checkpoint: babe668ffc41f1c8b90f9b1ed001e50d99fbde69
overall: READY-FOR-AUDIT
```

This re-QA is against the corrected committed checkpoint only. F1 is closed:
production `TeamsView` obtains the realm-bound `/v1/catalog` and `/v1/teams`
projections through kontord, while fixture data is restricted to injected tests
and the explicit offline-preview action (`TeamsView.tsx:80-124`). The API routes
are registered at `crates/kontor-api/src/lib.rs:211-217`; draft save and publish
are durable and cursor-bearing in `crates/kontor-store/src/teams.rs:45-131`,
with immutable revisions in migration `0021_teams_editor.sql`. The four MCP
ToolSpecs and parity assertions are present in `crates/kontor-mcp/src/registry.rs:449-519`
and `tests/contract/mcp_parity.rs:539-548`.

F2 is closed for the Teams production path. `teams.ts:1133-1135` now describes
the live `/v1/catalog` trust-boundary enforcement accurately, and the fixture
comment at `teams.ts:1229-1241` explicitly limits fixture use to tests/offline
preview. A tree search found no remaining Teams production `not wired` or
`stand-in` wording and no contradictory fixture claim. Unrelated wording still
exists in the Task/Board contract surfaces and a pilot test fixture; it does not
describe the Teams path and is not an ASMA-7854 residual.

### Acceptance criteria — live path

| AC | Verdict | Independent live-path evidence |
|---:|---|---|
| 1 | PASS | Teams is reachable through `App.tsx` with the attached realm client. The live view reads catalog and Teams projections before rendering an editor, and the responsive surface remains covered by the console suite and desktop/phone Playwright runs. |
| 2 | PASS | Live save calls `/v1/teams/drafts:save`; live publish saves then calls `/v1/teams/{team_id}/publish` (`TeamsView.tsx:196-215`). The store persists drafts, increments the shared projection cursor, assigns the next version, and inserts immutable revisions (`kontor-store/src/teams.rs:52-130`; migration `0021_teams_editor.sql`). The loopback test passed. |
| 3 | PASS | Production catalog data comes from `client.modelCatalog()` and is converted into the editor catalog (`TeamsView.tsx:89-98`). Provider/model/effort/context choices are constrained by that catalog; an unsigned promoted live catalog is refused before the template list/editor (`TeamsView.tsx:127-138`; `TeamsView.test.tsx:217-236`). |
| 4 | PASS | The live projection uses the shared chain validation, provider/route identity, cross-provider rung rules, pooled fallback, effort ladders, and verdict-capability checks. The model-chain core gate passed and the full console suite passed. |
| 5 | PASS | The live wire projection adopts server `resolved_policy` without inventing a client calculation (`TeamsView.tsx:1003-1015`). The daemon supplies realm/revision/cursor-bound resolved policy, including source, effective threshold, enforcement and capability; the loopback and live Teams tests cover the same projection. |
| 6 | PASS | The live view uses only the realm client projection/save/publish methods (`TeamsView.tsx:89-103`, `196-215`). No client-side provider probing, credential access, scheduler, or model-list discovery was added to the Teams path. |
| 7 | PASS | v1.2 context economics are applied to the live catalog: `resolveContext` clamps by model ceiling (`teams.ts:727-742`), cost is input-threshold-only with sourced/withheld values (`teams.ts:833-905`), and recommendation is the smallest covering class (`teams.ts:907-940`). Live clamp regression `TeamsView.test.tsx:637-650` passed. |
| 8 | PASS | Every cell carries provenance/gate state; catalog cells are refused at the live trust boundary and need bands remain draft-owned provenance checks. Server projections preserve realm/cursor and resolved-policy source. Review/provenance tests and the live refusal test passed; no unreviewed promotion is silently accepted. |

### Binding acceptance gates

All three binding gates pass in the live path. QA/Audit/integration may proceed on
the evidence below; no cited check was removed or failed.

1. **Telemetry-derived need bands, explicit role mapping, and no promotion — PASS.**
   `teams.ts:1506-1529` explicitly maps the ambiguous `lead` statistic to
   `architect` and `manualTestLead` to `qa`; these are not conflated. The
   telemetry citation, observation window, `fixture/needs-verification` state,
   and null review reference are present. `teams.test.ts:289-296` asserts the
   mapping, unpromoted state, disagreement with the seeded `deep` class, and
   the resulting `lean` recommendation. The exact mapping is `lead -> architect`
   and `manualTestLead -> qa`, not an unqualified `lead -> qa` claim.

2. **`validateCatalog` refusal at the `/v1/catalog` trust boundary — PASS.**
   The live catalog and Teams projection are loaded before the editor
   (`TeamsView.tsx:89-99`), then `validateCatalog` runs before any template list
   or editor renders (`TeamsView.tsx:127-138`). The live regression
   `TeamsView.test.tsx:217-236` kills validation removal: the mutant renders
   through to the editor, so the test fails to find the refusal alert and the
   absent-template-list assertion cannot pass.

3. **`(code, slot)` blocking-count deduplication — PASS.**
   `TeamsView.tsx:964-977` attaches slot identity and counts a Set keyed by
   `issue.code` plus `issue.slot`. The live projection regression
   `TeamsView.test.tsx:612-635` asserts `3 blocking`, not `6`. A live mutant
   adding a unique issue index to that Set was killed: the test rendered `6
   blocking` and failed its `3 blocking` assertion.

The requested clamp regression also passes at `TeamsView.test.tsx:637-650`.

### Verification gates

| Gate | Result |
|---|---|
| `apps/console: pnpm test` | PASS — 14 files, 278 passed |
| `apps/console: pnpm typecheck` | PASS — exit 0 |
| `apps/console: pnpm build` | PASS — 52 modules transformed, exit 0 |
| `apps/console: pnpm verify:api` | PASS — regenerated OpenAPI/schema diff empty |
| Playwright desktop + phone | PASS — 2 passed |
| `cargo test -p kontor-teams` | PASS — 43 passed, 0 failed |
| model-chain core check | PASS — 1 passed, 44 filtered |
| live loopback Teams projection check | PASS — 1 passed, 105 filtered |
| MCP parity | PASS — 11 passed, 0 failed |
| CLI | PASS — 14 passed, 0 failed |
| `cargo fmt --all -- --check` | PASS — exit 0 |
| relevant clippy (`api`, `daemon`, `mcp`, `store`, `teams`, `--all-targets`, `-D warnings`) | PASS — exit 0 |

### Mutation/regression evidence

All mutants were run one at a time in isolated `/private/tmp` copies; the
corrected checkpoint was not mutated and no mutant remains.

| Mutant | Result | Killing evidence |
|---|---|---|
| Telemetry circularity: architect observed band `76_000 -> 512_000` | KILLED | `teams.test.ts:295` expected `lean`, mutant produced `deep`. |
| Remove need-band provenance guard in `reviewSeat` | KILLED | `teams.test.ts:248-260` expected the three promotion provenance blocking codes, mutant produced `[]`. |
| Remove live `validateCatalog` enforcement | KILLED | `TeamsView.test.tsx:217-236` failed because the mutant rendered without the `/v1/catalog` refusal. |
| Remove `(code,slot)` dedup by adding a unique issue index | KILLED | `TeamsView.test.tsx:612-635` failed with `6 blocking` instead of `3`. |
| Remove context clamp (`effective=modelWindow`, `clamped`) | KILLED | `TeamsView.test.tsx:637-650` failed with `effective 512000`/`supported` instead of `effective 400000`/`clamped`. |

### Tree, lock, and exact residual

`Cargo.lock` is byte-identical to the corrected checkpoint:

```text
worktree: 03c9e37793cf1de5b1e385f47991784f4c725b35
HEAD:     03c9e37793cf1de5b1e385f47991784f4c725b35
```

The outer tree is clean. The submodule has only the pre-existing untracked
`evidence/ASMA-7854-AUDIT.md` and this requested untracked QA evidence file;
there are no source, test, generated-output, or lockfile changes.

Two non-failing integration qualifications remain explicit:

* The workspace manifest set currently makes Cargo's `--locked` mode refuse to
  update before running, although the normal requested Rust gates pass and the
  lockfile is restored byte-identically. Reconcile the workspace lock metadata
  before requiring a locked CI invocation.
* The correction handoff records a serialized migration-number collision with
  KON-23: this ticket uses `0021_teams_editor.sql`, while KON-23 also uses
  `0021_native_memory.sql`. Integration must merge KON-23 first, rename this
  migration to the next available number, update the migration registry/schema
  version, and rerun the full gates. This checkpoint itself has no tree or
  lockfile drift.

## Re-QA overall verdict

**READY-FOR-AUDIT**
