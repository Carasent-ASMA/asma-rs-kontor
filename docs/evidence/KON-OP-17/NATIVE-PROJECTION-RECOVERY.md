# KON-OP-17 / ASMA-7950 — native projection recovery receipt

Date: 2026-08-20
Branch: `fix/ASMA-7950-native-container-retitle-and-ecp-leadership-placement`
Status: implementation and local verification complete; merge/deployment receipt pending
Schema: 44 (no migration; schema 45 remains reserved for KON-OP-18)

## Operational gaps closed by this change

1. Imported legacy epic/task metadata now renders canonical native names:
   `Epic · <Jira epic> · <short title>`, `ECP · <Jira epic> · <short title>` and
   `TSW · <Jira issue> · <semantic short code>`. Unresolved template markers and
   topology UUIDs are never appended.
2. A native-root retitle is identified as a Paseo project operation. Paseo 0.4
   exposes no supported project rename, so preview/apply return the typed
   `unsupported_capability` result before any workspace/session lookup or
   mutation. Existing ESW project identities are preserved; recreation is not a
   repair path.
3. Canonical-cwd preparation refuses any existing differently titled workspace
   and any two-workspace ambiguity, including one exact plus one stale identity.
   It never creates another TSW beside title drift.
4. Core Team LSA/TPM hosted sessions may attach idempotently to the exact bound
   Directory/LocalCheckout ECP. Delivery roles remain forbidden there and still
   require the exact managed ticket worktree.
5. Runtime seat labels render `jira.epic` from the durable external Jira key.
   `kontor.project_id` intentionally remains the internal Kontor epic id. An
   Admin-only exact in-place reconciliation operation repairs already-bound
   delivery labels while preserving AgentRun, runtime binding, native session,
   provider session, container and generation identities.
6. An Admin may retire an exact queued/no-intent, idle, unarchived seat only
   when its native provider is configured unavailable and all binding/native/
   provider/AgentRun correlation evidence matches. Normal idle-seat reuse is
   unchanged. The linked successor still goes through ordinary admitted launch,
   and the caller must explicitly select the authorized Codex fallback route.

## Live identities preserved for operator follow-up

QNR ASMA-7675 remains parked and was not mutated by this repair.

- Kontor epic: `01a019c0-eee7-72a1-a8a7-7fff1ddce8f3`
- project: `01a0064a-e056-7603-9968-ef64fdaacb75`
- ESW node / native project:
  `01a01b25-c342-77e3-9802-fc4ccae3e8f0` / `prj_85aa32f2c4c4217f`
- ECP node / native workspace:
  `01a01b25-c343-7443-a1b0-145ca3ef6de5` / `wks_6f8d97404c7a18da`
- logical LSA / TPM SeatBindings:
  `01a01c3a-91ce-7c70-9b5e-30cbdb0737e1` /
  `01a01bfa-b4f4-7510-ad8c-59b08dfd85f6`

Provider-outage predecessors are evidence and must not be woken. Adam's
successor will execute the supported retirement/replacement flow after deploy:

- ASMA-7676 builder AgentRun `01a01ce5-bf36-7363-9143-dec616f2ba0b`,
  SeatBinding `01a01ce5-a451-74e3-9b50-431e4faf63d2`, runtime binding
  `01a01ce5-bf36-7363-9143-dedcfd4716ee`, native
  `50d35816-f80d-4ee6-8c4f-c5a3cb768c02`;
- ASMA-7676 inspector AgentRun `01a01ce5-f36d-7581-b175-b17c0e061f9a`,
  SeatBinding `01a01ce5-a45d-7480-bef5-82c1b40ce1f4`, runtime binding
  `01a01ce5-f36d-7581-b175-b18bea140b49`, native
  `acd47ff1-7e1a-4d78-93cb-9fec05358fec`;
- ASMA-7952 inspector AgentRun `01a01ca8-8c0a-7c72-8501-9896b9205ec0`,
  SeatBinding `01a01ca8-4ced-77a1-8447-2240d490ec8e`, runtime binding
  `01a01ca8-8c0a-7c72-8501-98a7dfd40c7e`, generation 1, native
  `5a80f097-358c-436d-817e-e3673f48e3df`.

## Bounded fallback evidence

The pre-existing direct-fallback OP-18 workspace `wks_46a223344a08e373` was
unbound, duplicated the authoritative cwd and contained zero agents. Because
Kontor had no orphan-native cleanup operation, the Adam successor performed one
bounded Paseo soft archive. Readback was `archivedAgentIds=[]` and
`removedDirectory=false`; the orphan disappeared from the active list while
authoritative `wks_442cc5fdb6c6b1a4` and all six seat labels stayed unchanged.
This is typed operational-gap evidence, not a normalized control-plane path.

## Regression and mutation evidence

The focused regressions cover canonical imported naming, exact-plus-stale cwd
duplication, native-project retitle refusal, real hosted Core Team launch in a
local ECP, external/internal label semantics, in-place label repair, and exact
provider-outage retirement/replacement. The deliberate mutation results are
recorded in `docs/evidence/KON-OP-17/MUTATION.md` as M19–M23.

Generated OpenAPI and console TypeScript artifacts are updated. Final local
verification on 2026-08-20 passed `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, the full Rust
workspace test suite, the OpenAPI contract test, generated console API parity,
recursive TypeScript typechecks, all 295 console tests, MCP parity and
`git diff --check`. PR/CI, merge, binary hash/PID and live daemon readback are
release receipts reported by the LSA after deployment.
