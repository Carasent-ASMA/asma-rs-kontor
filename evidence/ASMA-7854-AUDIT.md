# ASMA-7854 / KON-MVP-25 — final audit

```yaml
verdict: AUDITED_FALSE
outer_commit: 741e7f8e9c781fb80f274c12b19717194c6f0082
submodule_commit: babe668ffc41f1c8b90f9b1ed001e50d99fbde69
qa: evidence/ASMA-7854-QA.md — READY-FOR-AUDIT
design_gate: /Users/igor/kon-mvp-20-scratch/evidence/kontor-teams/RE-GATE-RECORD.md — COMPLIANT
read_only: true
```

## Checklist

| Area | Result | Evidence |
|---|---|---|
| AC-1 | PASS | Rail/view wiring: `apps/console/src/shell/NavRail.tsx:13-21`, `apps/console/src/shell/App.tsx:110-116`; responsive CSS `apps/console/src/console.css:474-716`; QA 14-file suite passes. |
| AC-2 | PASS | Immutable monotonic publish: `apps/console/src/state/teams.ts:499-514`; tests `apps/console/src/state/teams.test.ts:299-308`, `apps/console/src/views/TeamsView.test.tsx:515-525`. |
| AC-3 | FAIL for the ticket's live-catalog requirement | Catalog-constrained rules exist at `apps/console/src/state/teams.ts:552-812` and controls at `apps/console/src/views/TeamsView.tsx:523-647`, but `TeamsView` defaults to a local fixture rather than a live catalog; QA's tests prove the rules, not the required live boundary. |
| AC-4 | PASS | Cross-provider/pooled fallback and derived verdict: `apps/console/src/state/teams.ts:564-685`; tests cover same-provider refusal, pooled repeats and degraded lanes. |
| AC-5 | FAIL for API/CLI/MCP parity | The local preview shape exists at `apps/console/src/views/TeamsView.tsx:439-443` and is tested at `TeamsView.test.tsx:373-382`, but this checkpoint does not connect that preview to the same-realm API/CLI/MCP state. |
| AC-6 | PASS | Pure rules module contract: `apps/console/src/state/teams.ts:1-8`; no client probing, credentials or scheduler state. |
| AC-7 | PASS; supersession reconciled | Current AgentsRoom tracker task `1a4d4468-c566-4187-b3ce-3a1e4a434e10` / `jira:ASMA-7854` contains the amended v1.2 wording: smallest covering class, explicit charging basis, sourced metered dollars only, no cross-basis cheapest ranking, and task-minimum precedence. The re-gate's older open wording at `RE-GATE-RECORD.md:549-555` is superseded, not an unresolved scope cut. Code/tests: `teams.ts:696-938`, `:1003-1103`; precedence test `teams.test.ts:311-320`. |
| AC-8 | PASS for the required state/evidence contract | Provenance/gate reference and enforcement: `apps/console/src/state/teams.ts:42-177`; tests `teams.test.ts:169-285`. Need bands are intentionally `fixture/needs-verification`, cited and explicit; this is compliant and not a failed promotion requirement. |
| Zone C / hidden scope | FAIL — material limitation | The current tracker Zone C scope calls for `/v1/catalog`, commands and read projections, while this checkpoint uses a local fixture: `apps/console/src/views/TeamsView.tsx:61-70`, `apps/console/src/state/teams.ts:1217-1223`; the UI explicitly says it is not realm data at `TeamsView.tsx:97-100`. This is disclosed, so it is not silently disguised, but the committed tree does not provide the ticket's live API/CLI/MCP parity. |
| Design v1.2 | PASS, with the documentation finding below | The re-gate is `COMPLIANT`; unsupported values remain unpromoted and AC-7 is now closed by the current tracker amendment. |
| QA counts | PASS | Independently reproduced: `pnpm test -- --run` = 14 files / 272 tests; `pnpm typecheck` exit 0; `pnpm build` exit 0 / 52 modules; offline Rust recheck = 43 `kontor-teams` tests and 1 focused core test passed. |
| Tree / lock | PASS for source tree; expected evidence artifacts only | Outer `HEAD` and submodule pin match the audited commits. `Cargo.lock` is byte-identical: `03c9e37793cf1de5b1e385f47991784f4c725b35` for both `HEAD` and worktree. Submodule status contains only the requested untracked `evidence/ASMA-7854-QA.md` and this audit record; no staged/source/test/build/lock changes. |

## Binding gates

1. **Telemetry bands + lead mapping — PASS.** `teams.ts:1493-1519` cites the AgentsRoom statistics derivation/date, maps `lead -> architect` and `manualTestLead -> qa`, and keeps bands unpromoted. Test `teams.test.ts:288-296`; QA's telemetry mutant was killed.

2. **`validateCatalog` at `/v1/catalog` — PASS.** Validator `teams.ts:164-177`; trust-boundary refusal before editor construction `TeamsView.tsx:70-84`; regression `TeamsView.test.tsx:196-210`; QA's removal mutant was killed.

3. **`(code, slot)` blocking-count dedup — PASS by direct proof, not mutation coverage.** `TeamsView.tsx:868-881` keys a `Set` by `${issue.code}\0${issue.slot}`. The committed regression `TeamsView.test.tsx:527-547` supplies the same three defects through both paths and expects `3 blocking`, not `6`. An isolated no-edit proof constructed six duplicated keys and asserted `Set.size === 3`. No dedup mutant was seeded; mutation coverage is therefore not claimed complete.

## Findings requiring disposition

1. **F1 — live scope is not present in this checkpoint (blocking).** The current ticket's Zone C and goal require a catalog served through kontord plus API/CLI/MCP parity, but `TeamsView` defaults to `FIXTURE_CATALOG` and `teams.ts:1217-1223` calls it a stand-in. The disclosure prevents calling this hidden, but it remains an unfulfilled committed-tree scope/acceptance claim. `AUDITED_FALSE` is issued on this finding.

2. **F2 — stale executable-contract documentation (blocking for audit integrity).** `apps/console/src/state/teams.ts:1120-1123` says `validateCatalog` is “not wired into any runtime path,” while `apps/console/src/views/TeamsView.tsx:70-84` is the live enforcement path and binding gate. This is not acceptable harmless docs drift: it gives the opposite operational guidance for a mandatory trust boundary and requires correction. No code/docs edit was made under the audit's read-only constraint.

## Reconciliation notes

- The prior `PENDING` preflight's lock failure is superseded: the lock was restored and independently re-hashed equal to `HEAD`.
- The prior preflight's AC-7-open note is superseded by the current tracker/Jira-linked task body. AC-7 is closed under design v1.2.
- Telemetry bands remain intentionally unpromoted/needs-verification; the acceptance is derivation, citation and explicit `lead` mapping, all present. No promotion was expected or inferred.
- QA's mutation table covers telemetry, need-band provenance and `validateCatalog`; it does not cover deduplication. The direct proof above is the narrower evidence and is not reported as mutation coverage.

No production files, integration state or commits were changed by this audit.

## Re-audit — corrected committed checkpoint (2026-08-14)

### Verdict

**AUDITED_TRUE** for the corrected committed tree:

- outer commit `0b5086174ce822be721953ae4318f4467788b410`
- submodule commit `35f4c7e8c8f46f1f4f82875b095ed538e442e378`

The earlier `AUDITED_FALSE` record above remains the historical result for
`741e7f8` / `babe668`; its F1 and F2 are resolved by the corrected checkpoint.

### Acceptance and scope checklist

| Area | Result | Committed evidence |
|---|---|---|
| AC-1 | PASS | Live Teams view is wired through `apps/console/src/shell/App.tsx:49` and the navigation/view path; responsive UI is covered by the two Playwright cases. |
| AC-2 | PASS | Live draft save and publish use `/v1/teams/drafts:save` and `/v1/teams/{team_id}/publish` at `apps/console/src/views/TeamsView.tsx:196-215`; durable store and immutable revisions are at `crates/kontor-store/src/teams.rs:52-130` and `migrations/0021_teams_editor.sql`. |
| AC-3 | PASS | Production loads the realm-bound catalog through `client.modelCatalog()` at `apps/console/src/views/TeamsView.tsx:89-98`; `validateCatalog` refuses before templates/editor construction at `:127-138`, with live regression at `TeamsView.test.tsx:217-236`. |
| AC-4 | PASS | Shared validation covers cross-provider, pooled fallback, effort and verdict behavior in the committed teams state/tests. |
| AC-5 | PASS | Server-provided `resolved_policy` is consumed at `apps/console/src/views/TeamsView.tsx:1003-1015`; realm/revision/cursor projections, daemon loopback coverage, four ToolSpecs and MCP parity are present at `crates/kontor-mcp/src/registry.rs:449-519` and `tests/contract/mcp_parity.rs:539-548`. `pnpm verify:api` is an empty diff. |
| AC-6 | PASS | `TeamsView.tsx:89-103,196-215` uses realm-bound projections and save/publish only; no client probing, credential use, scheduler discovery or model discovery is present. |
| AC-7 | PASS | Current ticket/tracker amendment applies design v1.2: smallest covering class, explicit charging basis, metered dollars only when sourced, no cheapest cross-basis ranking, and task-minimum precedence. Implementation is at `apps/console/src/state/teams.ts:727-940`; live clamp regression is `TeamsView.test.tsx:637-650`. |
| AC-8 | PASS | Provenance, gate state, cited derivation and unpromoted `fixture/needs-verification` bands are enforced in `teams.ts:1506-1529`; the required explicit `lead` mapping is tested. |
| Zone C / hidden scope cuts | PASS | Corrected tree contains live `/v1/catalog`, `/v1/teams`, draft/publish registration at `crates/kontor-api/src/lib.rs:211-217`, durable revisions, production kontord projections, four ToolSpecs and parity. No contradictory live-scope omission remains. |
| F1 correction | PASS | Production is live; fixture usage is limited to tests/offline-preview (`TeamsView.tsx:111-121`). The corrected re-QA confirms no production “not wired”/“stand-in” wording for this feature. |
| F2 correction | PASS | `apps/console/src/state/teams.ts:1133-1140` now describes the live boundary accurately; the fixture comment at `:1230-1232` is explicitly test/offline-preview scope. The stale comment is corrected, not acceptable as docs drift. |
| Design v1.2 / re-gate reconciliation | PASS | The re-gate record remains `COMPLIANT`. Its older AC-7-open note at `RE-GATE-RECORD.md:549-555` is superseded by the current AgentsRoom/Jira amendment and is not an unresolved scope cut. |

### Binding gates

1. **Telemetry bands and Lead mapping — PASS.** `teams.ts:1506-1529` derives and cites the bands, keeps them explicitly unpromoted/`needs-verification`, and maps ambiguous telemetry `lead` to `architect` while `manualTestLead` maps to `qa`. The acceptance is derivation, citation and explicit mapping—not promotion. QA mutation-tested the circularity and provenance protections.

2. **`validateCatalog` at `/v1/catalog` — PASS.** The live catalog/projection is loaded before editor construction (`TeamsView.tsx:89-99`); validation refuses before templates/editor rendering (`:127-138`). The live removal mutant is killed by `TeamsView.test.tsx:217-236`.

3. **`(code,slot)` `blockingCount` dedup — PASS by direct committed proof.** `TeamsView.tsx:964-977` keys a `Set` by issue code and slot. `TeamsView.test.tsx:612-635` proves the duplicate paths count once, and the QA live unique-index mutant produces `6` instead of `3`. This is sufficient direct proof; no claim of mutation coverage is made for this gate because that mutant was not seeded in the QA run.

### QA reproducibility

The corrected QA evidence is `READY-FOR-AUDIT` and its committed-state counts reconcile as follows: console **278**, typecheck/build **0**, Playwright **2**, Teams **43**, model-chain **1**, loopback **1**, MCP parity **11**, and CLI **14**. Relevant formatting, API-generation and focused lint gates also pass; `pnpm verify:api` has no diff. The cited files and lines are present at the corrected submodule checkpoint.

### Tree, state and lock integrity

- Outer tree: clean at `0b5086174ce822be721953ae4318f4467788b410`.
- Submodule: detached at `35f4c7e8c8f46f1f4f82875b095ed538e442e378`; only the requested untracked evidence files are present (`evidence/ASMA-7854-AUDIT.md` and `evidence/ASMA-7854-QA.md`). No staged, source, test, build or lock work is present.
- `Cargo.lock` is byte-identical to the committed object: `03c9e37793cf1de5b1e385f47991784f4c725b35`.
- Git history confirms the outer pin and corrected submodule checkpoint; the cited implementation, tests and evidence files are contained in the checkpoint.

### Non-blocking integration qualifications

The standalone branch still records two integration-time items and they are not audit blockers here: Cargo `--locked` metadata reconciliation, and migration `0021_teams_editor` colliding with KON-23's `0021_native_memory` (integration renumbers Teams to `0022`, updates the registry/schema expectations, and reruns the gates). Independent provenance confirms the stale-lock refusal already exists on the shared pre-ticket base `3cf8221efb0b6497b1069b526b6960d5072f1127`, so it predates KON-23 and KON-25 and is the declared KON-20 one-lock integration responsibility. This does not waive final shipment: the merged archive must regenerate the lock and pass the locked workspace/clippy/test/license gates.

No production files, integration state or commits were changed by this audit; this durable re-audit section is the requested evidence update.
