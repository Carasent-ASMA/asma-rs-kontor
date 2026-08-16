# KON-OP-01 / ASMA-7870 — QA report

Date: 2026-08-16
Commit: `181c628` (merge of `origin/master` into the OP-01 branch)
Branch: `feat/ASMA-7870-kontor-operational-domain`
Frozen submodule: `_tools/asma-rs-kontor`

## Verdict

**PASS**, re-verified after merging `origin/master` (`5e38792`, OP-REQ-039
seat-attachment guardrails).

| Gate | Result |
| --- | --- |
| `cargo test --workspace` | 1272 passed, 0 failed, 7 ignored |
| `cargo test -p kontor-core` | 115 passed, 0 failed |
| `cargo test -p kontor-store` | 264 passed, 0 failed |
| `cargo test -p kontor-runtime-paseo` | 126 passed, 0 failed |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0, no diagnostics |
| `cargo fmt --all -- --check` | clean |
| Targeted mutation | 6 seeded, 6 killed |

The ignored tests are pre-existing live-harness tests, unchanged by this ticket.
The workspace count rose from 1264 to 1272 because the merge brought master's
eight OP-REQ-039 tests.

## Merge reconciliation (`181c628`)

`origin/master` and this branch both edited
`crates/kontor-store/src/repository.rs`. The two changes do not interact:
OP-REQ-039 concludes seat attachment from existing `agent_runs` rows and adds no
migration and no durable table, so it introduces no record that could carry a
classification. Seats and bindings are tier A under OP-REQ-037 either way, and
`tier_a_operational_tables_have_nowhere_to_store_a_classification`
(`schema_v1.rs:3038`) still holds.

Resolution was verified symbol-by-symbol rather than by eye: every symbol either
side added is present in the result with matching occurrence counts, and the
only textual losses were import lines that reflowed to absorb the other side's
new names. No migration-number collision — master touched no migration, so
`0025` and `SCHEMA_VERSION = 25` stand.

One inherited defect was fixed to make the branch green; see `OPEN-QUESTIONS.md`
OQ-OP-01-5.

## OP-01 acceptance criteria

Criteria 1–6 below were delivered by the predecessor commit `7314721` and are
re-verified green here; criteria 7–9 are this run's work.

1. **PASS — structural validation fails closed.** Invalid/cyclic parents,
   missing/duplicate roots, undeclared kinds, invalid projection-capability sets,
   cardinality violations, duplicate node/seat keys, unknown/duplicate role codes
   and free-form role strings are refused. `crates/kontor-core/src/spec.rs:392`
   onward; `crates/kontor-store/tests/operational_topology.rs`.
2. **PASS — published and snapshotted specifications are immutable.** `0023`
   `BEFORE UPDATE`/`BEFORE DELETE` triggers on `topology_specs` and
   `mini_project_topology_snapshots`.
3. **PASS — Operational default topology data.** `LSA` and `TPM` required, `SA`
   cannot satisfy the `LSA` cardinality, one TSW per ticket workspace regardless
   of seat count, `ASW`/`CSW` distinct, Seat absent from every node-kind list.
   `crates/kontor-profiles/fixtures/operational-domain.json`.
4. **PASS — a different valid kind vocabulary needs no kernel change.** Node
   kinds are specification data (`TopologyKindKey`), not Rust enums.
5. **PASS — `TSC` normalizes to `CSW` at import only; `PASE` is not seeded.**
   `crates/kontor-core/src/id.rs:531` (`parse_import`); new writes call `parse`
   and therefore cannot emit `TSC`.
6. **PASS — adaptive replay state survives restart/export/restore, ignores a
   replayed observation id and carries no scheduler policy.**
   `operational_topology.rs:151-201`; replay refusal asserted at `:173`.
7. **PASS — every OP-01-owned classifiable published document carries immutable
   `shareability`, classifier identity and provenance.**
   - Stored and read back across restart: `operational_topology.rs:35`.
   - Human override stored whole, identity preserved: `:243`.
   - Post-hoc reclassification refused through direct SQL: `:284`.
   - Unattributed override refused by the schema: `:316`.
   - Forged type-default class refused at the publish boundary: `:355`.
   - Defaults apply with no human prompt: `spec_validation.rs:1630`.
8. **PASS — tier-A operational rows refuse classification.** Domain refusal at
   `spec_validation.rs:1614`; the three tier-A tables have zero `shareability%`
   columns, asserted at `schema_v1.rs:3038`.
9. **PASS — old data opens unchanged; export/restore preserves the stamp.** A
   genuine v24 database with pre-classification documents opens, keeps its Realm
   identity and backfills the tier default with a NULL classifier:
   `schema_v1.rs:2952`. Export carries all three columns and is asserted in
   `operational_topology.rs:35`.

## Ownership boundary

`git status` after formatting and clippy shows changes confined to the OP-01
ownership list:

```
crates/kontor-core/src/repository.rs
crates/kontor-core/src/spec.rs
crates/kontor-core/tests/spec_validation.rs
crates/kontor-store/migrations/0025_document_shareability.sql
crates/kontor-store/src/backup/export.rs
crates/kontor-store/src/migrations.rs
crates/kontor-store/src/repository.rs
crates/kontor-store/tests/operational_topology.rs
crates/kontor-store/tests/schema_v1.rs
```

No file outside that list changed. One item of observed drift is reported in
`OPEN-QUESTIONS.md` (OQ-OP-01-3): running the workspace suite regenerates
`docs/evidence/KON-MVP-18/run-<id>/`, a Foundation-era pilot evidence directory.
It is left uncommitted because it belongs to KON-MVP-18, not OP-01. Its own
verdict is ACCEPT, 42 pass / 0 fail.

## Mutation ledger

Each mutant was seeded into the committed code, the named test was run, and the
mutant was reverted.

| # | Seeded defect | Expected to fail | Result |
| --- | --- | --- | --- |
| M1 | Tier A returns a default class instead of refusing | `tier_a_operational_state_refuses_classification` | KILLED |
| M2 | Tier-B default flipped to `kontor_local` | `each_classifiable_tier_has_a_default_so_work_never_stalls` | KILLED |
| M3 | `validate_for` stops checking the type-default class | `an_override_is_attributable_and_a_default_is_not` | KILLED |
| M4 | Attributability trigger never fires (`WHEN 0 AND …`) | `an_unattributed_override_is_refused_by_the_schema` | KILLED |
| M5 | Migration backfill default flipped to `kontor_local` | `documents_published_before_the_classification_existed_adopt_the_tier_default` | KILLED |
| M6 | `validate_for` dropped from both publish paths | `publishing_refuses_a_class_nobody_chose` | KILLED (survived first pass; see REVIEW-NOTES F-1) |

M6 initially **survived**, which is the finding recorded as F-1. The suite was
extended and M6 re-run against the new test before this verdict was written.
