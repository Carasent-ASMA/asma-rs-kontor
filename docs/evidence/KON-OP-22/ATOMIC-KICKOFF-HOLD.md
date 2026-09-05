# Atomic kickoff hold

Date: 2026-09-05

Status: implementation and local qualification complete; merge and live
promotion receipts are recorded separately when they exist.

## Operational gap

`epics:apply` created ready tasks under default-allow admission. The documented
kickoff procedure could only arm and then disarm after graph creation, leaving
a window in which the new governed epic was eligible. Omitting
`execution:arm` did not close that window.

This gap was discovered while previewing the publication-identity enforcement
initiative. The preview made no write, no fallback was used and the new epic
was not applied. The correction remains owned by KON-OP-22 / Jira `ASMA-8090`
because that task owns removal of Kontor control-plane dead ends and manual
fallbacks.

## Correction

`epics:preview` and `epics:apply` now accept an optional `initial_hold` object:

```json
{
  "held_by": "<project account-profile id>",
  "reason": "Kickoff hold until handoff"
}
```

Apply derives stable child idempotency keys, records an ordinary epic-wide
execution authorization and immediately revokes it. The grant and revocation
retain distinct command receipts and are returned in `initial_hold`.

The graph is ungoverned until the revoked authorization is durable. Since the
scheduler already refuses ungoverned epics, every interruption boundary is
closed:

1. before the grant, the epic is ungoverned;
2. after the grant and before revocation, the epic is still ungoverned;
3. after revocation, the covering hold blocks every task;
4. only then does apply install governance.

A retry converges the same child commands and authorization. A new idempotency
key cannot retrofit an initial hold onto an already governed epic, and preview
refuses to describe that operation as safe.

The MCP registry declares the nested object and therefore exposes it through
both MCP and the generated CLI. OpenAPI and the console client types were
regenerated from the same DTOs. Authorization projections now carry the grant
and revocation receipts, actors and revocation reason required for durable
readback.

## Qualification

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed; the loopback suite reported 274 passed and
  one superseded case ignored, and the new hold test passed.
- `cargo audit`: passed with the repository's existing allowed warnings.
- `cargo deny check`: passed.
- `pnpm --filter kontor-console verify:api`: passed.
- `pnpm -r typecheck`: passed.
- `pnpm -r test`: passed, 300 console tests.

The focused test proves preview shape, separate grant and revocation receipts,
an empty scheduler-ready set, `authorization_blocked` for every task, projection
readback and stable replay. It also proves that late preview/apply attempts are
refused.

## Mutation proof

The implementation was temporarily changed to return the live grant without
calling the disarm operation. The focused loopback test failed on the missing
`revoked_at` and displayed the still-live authorization. The disarm call was
restored and the same test passed. No mutation remains in the tree.
