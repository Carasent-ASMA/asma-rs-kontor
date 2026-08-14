# KON-MVP-20 negative-boundary proofs

Archive anchor: `5cc0e223e8f297f551bb521c580508395620d432`. The full
workspace suite and every named killer below passed on the committed archive;
the corresponding mutant in `MUTATION_LEDGER.md` then made it fail.

| Boundary | Committed deterministic proof | Mutant |
|---|---|---|
| Runtime consistency / restart generation | `tests/contract/runtime_adapter.rs:1223,1443`; missing sessions become lost-contact and a generation restart invalidates the stale binding. | L01, L13 |
| Arbitrary profile authority | `crates/kontor-policy/tests/guardrail_rules.rs:375,797`; custom/renamed profiles decide only through declared authority. | L02 |
| Seed-id branching | `crates/kontor-scheduler/tests/no_seed_branching.rs:12,87` plus renamed-profile coverage at `crates/kontor-profiles/tests/profile_contract.rs:771`. | L03 |
| Persona self-approval | `crates/kontor-policy/tests/guardrail_rules.rs:813,825`. | L04 |
| Event dedup and intake authorization | E2E intake replay at `tests/e2e/pilot_sections/domain.rs:450`; internal receipt validation at `crates/kontor-core/tests/spec_validation.rs:776`; approval lineage and no-dispatch boundaries at `crates/kontor-store/tests/intake_lineage.rs:638,1207`. | L05, L06 |
| No external-workflow hardcoding | Alternate project fixture is bound at `crates/kontor-integrations-asma/tests/contract.rs:54`; transition-by-destination and shipped-source invariants start at `:694`. | L07 |
| Terminal assignee preservation | `crates/kontor-core/tests/ticket_policy.rs:910`; absent outbound fields are omitted, never nulled, at `crates/kontor-integrations-asma/tests/contract.rs:1084`. | L08 |
| Three-zone ownership/privacy | `kontor-core/src/ticket.rs:433` refuses outward private fields; E2E `domain.privacy-zones` is asserted at `tests/e2e/pilot_sections/domain.rs:2051`. | L09 |
| Optional/unrestricted calendar admission | `crates/kontor-scheduler/tests/ready_batch.rs:601,636` keeps authorization mandatory; `crates/kontor-store/tests/repository_roundtrip.rs:2722` keeps no-calendar unrestricted rather than closed. | L10 |
| No direct client-to-runtime access | MCP schemas are scanned at `tests/contract/mcp_mutants.rs:103,224`; E2E client bundle scan is `tests/e2e/pilot_sections/session.rs:947`; daemon DTO/row boundary is `crates/kontor-daemon/tests/loopback_api.rs:1558`. | L11 |
| No transcript/token persistence | `crates/kontor-store/tests/event_replay.rs:1219` and backup/export canaries. | L12 |
| Session-gap refetch | Store epoch/gap proof at `crates/kontor-store/tests/event_replay.rs:1084`; runtime strict-after proof at `tests/contract/runtime_adapter.rs:1690`; Paseo merge/refetch guard at `crates/kontor-runtime-paseo/src/adapter.rs:3149-3182`. | L13 |
| Client projection parity | Equal/older cursors are duplicates at `apps/console/src/state/control.test.ts:102,132`; stale/unreachable never becomes finished at `:205`; authoritative cursor check is `apps/console/src/state/control.ts:222`. | L14 |

Additional corrective boundaries are green in the same archive:

- observer/operator/admin refusal and one-credential MCP routing:
  `crates/kontor-daemon/tests/mcp_journey.rs:289-697`;
- MCP cardinality/parity and absence of direct store/runtime business logic:
  `tests/contract/mcp_cardinality.rs`, `tests/contract/mcp_parity.rs`, and
  `tests/contract/mcp_mutants.rs`;
- memory revision, approval, FTS, Context Pack, cutover, receipt, and project
  isolation: `crates/kontor-store/src/memory.rs:679-1110` and ML01-ML13;
- API secret/runtime/transcript projection refusal:
  `crates/kontor-daemon/tests/loopback_api.rs:1558`.

The archive gate ran 1,238 Rust test successes (including doctests) and the
console ran 278 Vitest tests; all passed. No test is credited solely because a
source scan exists: each safety condition also has an observable domain, row,
DTO, cursor, or runtime-effect witness in the mutation ledger.
