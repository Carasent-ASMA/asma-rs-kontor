# KON-OP-01 / ASMA-7870 — QA report

Date: 2026-08-16
Commit: `b367683` (ECP + code help, on the `181c628` master merge)
Branch: `feat/ASMA-7870-kontor-operational-domain`
Frozen submodule: `_tools/asma-rs-kontor`

## Verdict

**PASS**, covering the shareability stamp (OP-REQ-037), the merged OP-REQ-039
seat-attachment guardrails, and the amended ECP / code-help boundary
(OP-REQ-040/041).

| Gate | Result |
| --- | --- |
| `cargo test --workspace` | 1280 passed, 0 failed, 7 ignored |
| `cargo test -p kontor-core` | 115 passed, 0 failed |
| `cargo test -p kontor-store` | 264 passed, 0 failed |
| `cargo test -p kontor-teams` | 44 passed, 0 failed |
| `cargo test -p kontor-profiles` | 31 passed, 0 failed |
| `cargo test -p kontor-runtime-paseo` | 126 passed, 0 failed |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0, no diagnostics |
| `cargo fmt --all -- --check` | clean |
| Targeted mutation | 11 seeded, 11 killed |

The ignored tests are pre-existing live-harness tests, unchanged by this ticket.
The workspace count went 1264 → 1272 when the merge brought master's eight
OP-REQ-039 tests, then → 1280 with the eight ECP/code-help tests added here.

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

Criteria 1–6 were delivered by the predecessor commit `7314721` and are
re-verified green here. Criteria 7–8 are the amended ECP and code-help
boundary (OP-REQ-040/041); 9–11 are the shareability work.

1. **PASS — structural validation fails closed.** Invalid/cyclic parents,
   missing/duplicate roots, undeclared kinds, invalid projection-capability sets,
   cardinality violations, duplicate node/seat keys, unknown/duplicate role codes
   and free-form role strings are refused. `crates/kontor-core/src/spec.rs:392`
   onward; `crates/kontor-store/tests/operational_topology.rs`.
2. **PASS — published and snapshotted specifications are immutable.** `0023`
   `BEFORE UPDATE`/`BEFORE DELETE` triggers on `topology_specs` and
   `mini_project_topology_snapshots`.
3. **PASS — Operational default topology data.** One TSW per ticket workspace
   regardless of seat count, `ASW`/`CSW` distinct, Seat absent from every
   node-kind list. `crates/kontor-profiles/fixtures/operational-domain.json`.
   The `LSA`/`TPM` half of this criterion was restated by OP-REQ-040 and is now
   evidenced as criterion 7.
4. **PASS — a different valid kind vocabulary needs no kernel change.** Node
   kinds are specification data (`TopologyKindKey`), not Rust enums.
5. **PASS — `TSC` normalizes to `CSW` at import only; `PASE` is not seeded.**
   `crates/kontor-core/src/id.rs:531` (`parse_import`); new writes call `parse`
   and therefore cannot emit `TSC`.
6. **PASS — adaptive replay state survives restart/export/restore, ignores a
   replayed observation id and carries no scheduler policy.**
   `operational_topology.rs:151-201`; replay refusal asserted at `:173`.
7. **PASS — the Operational default requires one ECP holding distinct `LSA` and
   `TPM` SeatBindings; LSA/TPM never appear as node kinds (OP-REQ-040).**
   Seeded vocabulary is exactly `PSW QSW ESW ECP TSW ASW CSW`
   (`operational_domain.rs:42`). ECP is `native_child + session_host`, never
   `native_root`, cardinality exactly one under ESW (`:57`), so the default
   claims no nested Paseo workspace. LSA, TPM, SA and SEAT are absent from the
   kind list and present as role codes (`:92`); the store round trip binds both
   control seats to one ECP node (`operational_topology.rs:117`). `SA` cannot
   satisfy the `LSA` slot (`:117`).
8. **PASS — every published topology kind and role code has non-empty code-help
   full name and meaning (OP-REQ-041).** All seven kinds and all 56 roles carry
   code, full name, meaning, category and lifecycle
   (`operational_domain.rs:141`); a full name equal to its own code, or a role
   meaning equal to its title, fails. `TSC` is `compatibility` and `PASE`
   `retired`, explained but never declarable (`:176`); an unknown code resolves
   to `None` rather than a guess (`:196`).
9. **PASS — every OP-01-owned classifiable published document carries immutable
   `shareability`, classifier identity and provenance.**
   - Stored and read back across restart: `operational_topology.rs:35`.
   - Human override stored whole, identity preserved: `:243`.
   - Post-hoc reclassification refused through direct SQL: `:284`.
   - Unattributed override refused by the schema: `:316`.
   - Forged type-default class refused at the publish boundary: `:355`.
   - Defaults apply with no human prompt: `spec_validation.rs:1630`.
10. **PASS — tier-A operational rows refuse classification.** Domain refusal at
   `spec_validation.rs:1614`; the three tier-A tables have zero `shareability%`
   columns, asserted at `schema_v1.rs:3038`.
11. **PASS — old data opens unchanged; export/restore preserves the stamp.** A
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
crates/kontor-profiles/fixtures/operational-domain.json
crates/kontor-profiles/tests/operational_domain.rs
crates/kontor-store/migrations/0025_document_shareability.sql
crates/kontor-store/src/backup/export.rs
crates/kontor-store/src/migrations.rs
crates/kontor-store/src/repository.rs
crates/kontor-store/tests/operational_topology.rs
crates/kontor-store/tests/schema_v1.rs
```

All of these are inside OP-01's declared ownership: the `kontor-core`
domain files, the topology/role-catalog data boundary in `kontor-profiles` and
its fixture, and the `kontor-store` migration, repository and export. The one
file outside it, `crates/kontor-store/src/repository.rs`'s inherited clippy fix,
is explained in OQ-OP-01-5 — and that file is itself OP-01-owned.

`kontor-teams` was not touched (OQ-OP-01-2). The KON-MVP-18 evidence bundles
(OQ-OP-01-3) were committed separately in `49ab481` at the TPM's direction.

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
| M7 | ECP cardinality relaxed to optional per ESW | `one_epic_control_plane_sits_under_each_epic_workspace` | KILLED |
| M8 | `LSA` reinstated as a topology-node kind | `control_roles_are_seat_bindings_and_never_topology_kinds` | KILLED |
| M9 | ECP claims `native_root` (a nested Paseo project) | `one_epic_control_plane_sits_under_each_epic_workspace` | KILLED |
| M10 | A role's meaning replaced by its own title | `every_seeded_code_carries_server_owned_help` | KILLED |
| M11 | Lifecycle/declarability rule removed from `validate` | `a_specification_cannot_declare_a_non_current_code_as_a_usable_kind` | KILLED |

M6 initially **survived**, which is the finding recorded as F-1. The suite was
extended and M6 re-run against the new test before this verdict was written.
