# KON-MVP-20 final mutation ledger

Validated 2026-08-14 from a disposable `git archive` of submodule commit
`5cc0e223e8f297f551bb521c580508395620d432` (outer commit
`f9e341440b140ec4b94fbfeadfe5f52fd8e0ea89`). The source archive SHA-256 was
`3ae9c7ae345072e909abcbd6f7464af5b8cc06d80d1b7941802d9de207f9572a`.

## Result

- Score: **27/27 KILLED (100%)**.
- Survivors: **none**. Equivalent mutants: **none**.
- Every mutant was seeded alone into a fresh extraction. Its killer passed on
  the unmodified extraction first. Compilation-only failures were not counted.
- Each recorded failure is a deterministic test assertion or database/domain
  refusal. The extraction was destroyed before the next mutant.

### Baseline-to-mutant protocol

For each ID, the command below ran against the fresh unmodified extraction
first and exited `0`; after applying only that row's patch, the identical
command exited `101` for Rust tests or `1` for L14's Vitest command. The source
archive was re-extracted rather than restored in place.

| IDs | Baseline and mutant command selector |
|---|---|
| L01, L13 | `cargo test -p kontor-tests-contract --test runtime_adapter --locked` (named reconciliation/gap case) |
| L02, L04 | `cargo test -p kontor-policy --test guardrail_rules --locked` (named authority/persona case) |
| L03 | `cargo test -p kontor-scheduler --test no_seed_branching --locked` |
| L05 | `cargo test -p kontor-store --test intake_lineage --locked` (named replay case) |
| L06 | `cargo test -p kontor-core --test spec_validation --locked an_intake_decision_must_be_internally_consistent` |
| L07 | `cargo test -p kontor-integrations-asma --test contract --locked` (workflow/source invariants) |
| L08 | `cargo test -p kontor-core --test ticket_policy --locked preserve_never_clears_a_terminal_assignee` |
| L09 | `cargo test -p kontor-tests-e2e --test pilot --locked` (`domain.privacy-zones`) |
| L10 | `cargo test -p kontor-scheduler --test ready_batch --locked an_unrestricted_calendar_still_needs_an_authorization` |
| L11 | `cargo test -p kontor-tests-contract --test mcp_mutants --locked no_tool_names_a_runtime_endpoint_or_a_provider` |
| L12 | `cargo test -p kontor-store --test event_replay --locked transcript_and_token_deltas_are_rejected` |
| L14 | `pnpm --filter kontor-console test -- control.test.ts` |
| ML01, ML03, ML07, ML09 | `cargo test -p kontor-store --lib --locked` with respectively `reproposal_never_resets_the_aggregate_revision`, `two_approvals_leave_exactly_one_current_revision`, `frozen_revision_hash_is_the_approved_stored_hash`, or `proposal_never_enters_fts_before_approval` |
| ML02, ML04-ML06, ML08, ML13 | `cargo test -p kontor-store --lib --locked ledger_conflicts_filters_rebuilds_and_freezes_context` |
| ML10, ML11 | `cargo test -p kontor-store --lib --locked cutover_is_frozen_hashed_transactional_and_idempotent` |
| ML12 | `cargo test -p kontor-store --test backup_export --locked an_import_mints_a_destination_receipt_and_replays_no_source_receipt` |

## Legacy and negative-boundary mutants

| ID | Safety claim and seed | Patch SHA-256 | Deterministic oracle and exact red result | Non-equivalence witness | Result |
|---|---|---|---|---|---|
| L01 | Missing native session/generation change becomes terminal in `kontor-runtime/src/observation.rs:312` | `aec465cbeab580e02122b92a0b543db1375e14282ee5a87dbd7a01ad09042ed6` | `runtime_adapter::reconcile_classifies_missing_orphan_and_adoptable_sessions`, `runtime_adapter.rs:1278`: left `Terminal { Succeeded }`, right `LostContact` | Same missing native binding; only the safety classification changes terminal state. | KILLED |
| L02 | Authorize by role/profile name instead of declared authority in `kontor-policy/src/evaluator.rs:407` | `d3b633a4f22adc413196b0af0ab8a70b562e4c9f41d1b61c46d5fd3c0bfc7cad` | `guardrail_rules.rs:797`: left `Pass`, right `Block/RoleNotAuthorized` | Same receipt and custom profile; renamed role gains authority only in mutant. | KILLED |
| L03 | Known seed/profile-id branch in scheduler readiness | `277a316f046e23470ab776578e34f28f15357e5279031beb3663f3ae5d9e5e83` | `no_seed_branching.rs:87`: source invariant finds the injected identity literal | Same readiness inputs; changing only the profile id changes the mutant decision. | KILLED |
| L04 | Simulated persona approves its own gate in `kontor-policy/src/evaluator.rs:394` | `b97724ed45413e98cac90b02a255a2ecc333f6e9b0187953ac5c931cd02bd954` | `guardrail_rules.rs:823`: left `Pass`, right `PersonaSelfApproval` | Same persona receipt; only self-approval guard is removed. | KILLED |
| L05 | Replay bypasses intake decision lookup/dedup in the store intake boundary | `bb53d0d05e8c0f38535ef982f8456877b0af073251d98723971ee06a9411e5da` | `intake_lineage.rs:615`: replay reaches uniqueness conflict instead of returning the first receipt | Same envelope and idempotency key; mutant attempts a second graph write. | KILLED |
| L06 | Approval without internally matching evidence in `kontor-core/src/spec.rs:2817` | `89c6335751697f9e834c5bbb40425d6557416bafda74dcb331d8986bc20e9ca3` | `spec_validation.rs:781`: approval without evidence is accepted | Same approval; only the evidence-authority condition changes. | KILLED |
| L07 | Generic reconciliation branches on an ASMA/Jira status name | `3e3d8895165f7f29b4127bb8afaac9150f84cb6d3cc1f3796d053a21b909a5b7` | `kontor-integrations-asma/tests/contract.rs:687`: shipped-source scan detects injected `Ferdig` | Equivalent workflow fixtures with different names diverge only in mutant. | KILLED |
| L08 | Absent terminal assignee is projected as unassign in `kontor-core/src/ticket.rs` | `75f31ee5e9cf2cc066018715ecacc197628a2ee80d617cba8d065028cec611c7` | `ticket_policy.rs:932`: left `Transition::Unassign`, right `NoOp` | Same terminal ticket with absent projection; mutant clears an existing owner. | KILLED |
| L09 | Private Zone C projection guard removed at `kontor-core/src/ticket.rs:433` | `2f168d2cc9ca22305d9da975d9b95e13e65e423784f4b45e78ad96451118d05d` | E2E `domain.privacy-zones` (`pilot_sections/domain.rs:2051`) fails | Same outbound private mapping; baseline refuses, mutant serializes it. | KILLED |
| L10 | Unarmed task bypasses the first scheduler authorization blocker at `kontor-scheduler/src/ready.rs:324` | `a03144656cb88bbdbb764429dd2dbafb4b0cb4ef011ea47e5a6c5c2b5393e841` | committed KON-09 killer `ready_batch.rs:661`: left only contention, right authorization plus contention | Same unarmed, contending task; mutant reports/uses contention without retaining the authorization blocker. | KILLED |
| L11 | MCP schema accepts a direct runtime endpoint/provider | `dc7aa0a55311858cd8d5fb9b6bffb9a578f9192f9a50a83f8699ae34b14e358c` | `mcp_mutants.rs:63`: schema source scan reports the direct-runtime property | Same client request; mutant can select a runtime address outside kontord. | KILLED |
| L12 | Transcript/token delta passes the durable event guard | `f030b006595cd5a127b6d54d355d13afbebf597a2a2dd0e5a8cfc6261b73bcc8` | `event_replay.rs:1241`: transcript reaches the durable log | Same session-content delta; only persistence admission changes. | KILLED |
| L13 | Epoch/sequence gap is accepted in the timeline guard | `138bd77d908a37784b1ba5842b9f301475de530ca684a048db9e01a8c437c206` | `runtime_adapter.rs:1690`: forward sequence `5` is accepted instead of refetch | Same cursor and event; mutant advances across a missing sequence. | KILLED |
| L14 | Console accepts an inclusive duplicate cursor (`<=` to `<`) at `apps/console/src/state/control.ts:222` | `9d3938993e5adbe0a75bf993450b6ea8126ae68abdfeb4b083ed75268b592ab6` | `control.test.ts:102,132`: expected `duplicate`, got `applied` (2 failed, 47 passed) | Same projection event and cursor; equal cursor mutates client state only in mutant. | KILLED |

## Memory-ledger mutants

All source seeds below are in `crates/kontor-store/src/memory.rs`; all named
line numbers are the committed killers in that file unless another file is
shown.

| ID | Safety claim and seed | Patch SHA-256 | Deterministic oracle and exact red result | Non-equivalence witness | Result |
|---|---|---|---|---|---|
| ML01 | Proposal UPSERT overwrites aggregate revision | `d4e2a4fa23e14f21301e8d23d8987d60d19ad03f62c7912e23da7fb456cb3bcc` | committed KON-23 killer `memory.rs:799`: left `(0,2)`, right `(2,2)` | Same two proposals; mutant resets aggregate state instead of advancing it. | KILLED |
| ML02 | Remove expected-revision check (`memory.rs:145`) | `2c512c59b6a601bbbd07b3d4b25a853b9ad6f217be3656b21c4064a15a3381cb` | `memory.rs:702`: stale proposal is not refused | Same stale revision; mutant writes, baseline returns conflict. | KILLED |
| ML03 | Allow two current approvals | `1c917962f4a5fcc4eea24e98a81e9c6891251ee64f59b6843cc9b3191cd0a7da` | committed KON-23 killer `memory.rs:844`: left `2`, right `1` | Same item/revisions; mutant leaves two approved-current rows. | KILLED |
| ML04 | Draft revision injected into approved list | `a79e30895e8e1d64ddabafbf4d1ed008499b230eb892a500ed2f7ba729527145` | `memory.rs:705`: `InvalidColumnType`/NULL from the injected draft | Same list query; mutant exposes a row with no approved current revision. | KILLED |
| ML05 | Tombstone exclusion removed | `913247892bed1db4f8f3c230b83bbf85459435bc13897bc2787cc77c8cbeb0e6` | `memory.rs:758`: tombstoned item remains visible | Same approved item plus tombstone; mutant returns it. | KILLED |
| ML06 | Replay re-queries/rebinds a Context Pack (`memory.rs:386`) | `a06917dd9b03d34e3cbd86d27e6a1d87daeb9c705fe9f1f1d2fa7fbffde1f1a3` | `memory.rs:742`: UNIQUE `memory_context_bindings` failure | Same run/cursor; mutant attempts a second binding rather than replaying frozen bytes. | KILLED |
| ML07 | Omit frozen revision content hash | `73e85494aae224368a7a86653584c02356822374ab8eeefa32752eaab9b2fb12` | committed KON-23 killer `memory.rs:882`: frozen hash mismatch | Same approved revision; mutant Context Pack no longer attests its bytes. | KILLED |
| ML08 | Trust first FTS hit instead of authoritative approved/current match | `42df4c3aa8d9b3da5a9d40b54ee91fe0a80758b732b528211da607c3980d1c63` | `memory.rs:758`: injected non-authoritative/tombstoned hit escapes the guard | Same FTS rows; ordering changes only the mutant result. | KILLED |
| ML09 | Index proposal before approval/redaction | `a480a4a75d3d91937f44a11a262d2521b9cb2e48dd06b687f591eeceb4f57d1d` | committed KON-23 killer `memory.rs:922`: proposal search count left `1`, right `0` | Same unapproved document; mutant makes it searchable. | KILLED |
| ML10 | Permit duplicate import manifest | `2e34cceaf08c3a2f4c94fabba006721fdeeb95f0eaddd8cba3cf1bf2b3c4da39` | `memory.rs:1095`: UNIQUE `memory_import_manifests` failure | Same source/export hash imported twice; mutant attempts a second import. | KILLED |
| ML11 | Permit post-cutover AgentsRoom dual write | `a0a93e32b92e6a6ed06a2a07ecf7590078b729ef52de419096373c1c3222cc89` | `memory.rs:1042`: authority assertion fails | Same post-switch write; baseline refuses, mutant persists. | KILLED |
| ML12 | Omit memory import/export receipt | `1b6ba97ff849e10c11fcf97a3f0085c8c9eb4db0ad5d1f85f3486a75666fe486` | `backup_export.rs:470`: receipt count left `0`, right `1` | Same export/import; mutant leaves no audit receipt. | KILLED |
| ML13 | Remove both project filters from list and revision-history reads | `2522ce2a95c6cada6663876ccf6e0848ca1f14065813da4b7d4303f755c070eb` | `memory.rs:723`: “another project cannot retrieve it” | Same item id in two projects; mutant crosses the project/realm boundary. | KILLED |

ML13 deliberately seeds the whole two-layer project boundary. Removing only one
filter is masked by the other and is not the claimed defect; it was not scored.

## Cleanup proof

Only the clean `base/` archive extraction remained after the run. No mutant
extraction and no mutation/test process remained. The validation checkout was
never a mutation input. Its three preserved stale source files retained SHA-256
`30c583123ef488a191354f50d7999bf99a100e3666f52c251f545e6c25ebad46`,
`c1de6cf7bee4fb0b190f9dd1b6419826c11d3270c4182e30f48e5e6b72c80495`,
and `21c2974b423f3605fa3797bc528f6ecbf1e86f026b8c35d4391c27c874d8cfa5`;
they were not read as archive inputs, edited, staged, or committed.
