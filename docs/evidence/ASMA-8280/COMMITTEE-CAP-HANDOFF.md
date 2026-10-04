# ASMA-8280 formal Committee cap — handoff

- Date: 2026-10-02
- Task: ASMA-8280 / TASK-002, narrow Committee cap source correction (TPM turn 15 and the LSA continuation)
- Author: implement seat `462cd93d-30ec-4ed0-9841-3687612da5e0`, session `6db559f1`, `claude-work`/`claude-opus-5-5` at xhigh
- Workspace: `wks_a284b5d96a0ea32b`; branch `feat/ASMA-8280-share-activated-yaml-policy-across-orchestration-modes` (local, not pushed)
- Base: `545b44a1a693b3cc054bf453709a1dc4149b7440` (tree `fb0f9c87`). Every ancestor is unchanged.
- Status: **uncommitted.** One test is red at the boundary in "Callback boundary" below. Everything else is green at the final source hashes.

This record is implementation evidence and a handoff. It is not verification and does not close TASK-002.

## What the correction does

The formal Independent Review (template `01991c00-0000-7000-8000-000000000001`) now holds its whole Committee to:
- at most one Anthropic (Claude) seat, reviewers and judge counted together;
- verdict rungs 1–2 only.

Both orchestration modes enforce this in the one shared allocator. Reviewer vendor diversity still binds reviewers only, so the judge may share a reviewer's vendor. Every other Committee and every other joint request is allocated exactly as before.

| Piece | Source | What it does |
| --- | --- | --- |
| Shared allocator | `crates/kontor-fleet/src/allocation.rs` | Adds `allocate_constrained` with `AllocationConstraints{slots, vendor_cap, max_rung}`.<br>- A governed slot cannot take: a step beyond `max_rung` (`rung_beyond_verdict`); a step of `0`, meaning no authoritative rung (`rung_unknown`); or, under a cap, a route whose maker is unnamed (`vendor_unknown`).<br>- The exhaustive search counts every governed slot holding the capped vendor and backtracks. A passed-over capped route is `vendor_cap_reached` with `conflicts_with` naming a holder. A cap-only failure is `vendor_cap_exceeded`, with every slot unselected.<br>- `allocate` is `allocate_constrained` under empty constraints. A generic receipt has no `constraints` key.<br>- `committee_constraints(template_id, slots)` is non-empty only for the formal template: Anthropic at most 1, `max_rung` 2. `committee_template_of(binding_key)` parses `committee/<template>/<slot>` exactly. |
| Local adapter | `crates/kontor-fleet-activation/src/lib.rs` `Activated::allocate` | If any slot's binding names the formal protocol, every slot of the request is held to its constraints, so rebinding one seat cannot relax the cap. The request shape is unchanged and closed; it has no field that could carry or relax a constraint. |
| Governed adapter | `crates/kontor-daemon/src/applications.rs` `allocate_committee`, `formal_committee_candidates` | Constraints come from the selected template's identity. Formal candidates state only what an authoritative source says (rulings B and C below). The freeze refuses `vendor_cap_exceeded` `placement_blocked` with its own rule, before any run, seat, receipt or native effect. |
| Restored | `crates/kontor-fleet/src/lib.rs` | `independence_key` is byte-identical to the base. The interim prefix-based `maker_of` was removed (ruling C). |

## LSA rulings as implemented

- **A — the immutable v1 template.** `01991c00-…0001@1` is untouched and reads back with its bundled hash.
  - Its own unbound routes seat two Claude seats. With no selected policy no route names a maker, so the Committee is refused (`a Committee slot has no currently admissible governed route`).
  - With a policy that lists those routes but binds no slot, it is refused by the cap, with the new rule.
  - Both refusals happen before any run, receipt or native call: `without_a_compliant_fleet_the_formal_committee_is_refused_before_any_effect`, the converted LF-04 no-fleet test.
  - Formerly passing formal fixtures now write an explicit compliant selection, `formal_review_fleet_yaml()`. It binds only the judge, to the one-step `codex-sol` chain; reviewer A stays on the template's `claude-work` Opus and reviewer B on its `codex-work` Sol.
  - The fleet-bound LF-04 reviewer test binds its judge the same way. The initial-recovery test writes a policy that names every route's maker and binds nothing.
- **B — ranks.** Preference order is the admission's own order (bound chain or template first, then accepted initial recovery), and it is not the source ordinal.
  - A bound chain route keeps its chain step and place in the step. Its account expansion is not a rung.
  - An unbound template or accepted recovery route takes its own ordinal as its rung; expansions share it, and a rung of 3 or deeper gets no verdict. The `declared + rank` arithmetic is gone.
  - On a bound slot, a recovery route cannot add to the chain or relabel a rung of it (A-08). These all state rung `0` and are refused, typed `rung_unknown`: a route claiming a chain place it does not hold; chain provenance on an unbound slot; any other source; a rank of 0.
  - Receipts keep `source`, `rank` and `profile_hash` apart from `fleet_provenance{step, sub_step, vendor}`.
- **C — makers.** A maker is only the selected policy's exact model vendor at its hash, for that exact account alias in a domain the policy lists. `fleet.vendor_of`, and `FleetRoute.vendor` for chain routes, are the only sources.
  - No provider family, alias spelling, role catalog or independence key names a maker. The governed model catalog has no vendor field, so it names none.
  - An unnamed maker is ineligible for every capped slot, judge included. The generic path keeps its legacy independence keys.
- **D/E.** Covered by the receipts below.

## Receipts (module checkout, final source hashes in `/tmp/asma-8280-committee-cap-mutants-462/final.sha`)

Every run used toolchain 1.97.1 with `--offline --locked -j 2`, `env -i`, a private cloned `CARGO_HOME`, a private target and no sccache.

| Suite | Result |
| --- | --- |
| `kontor-fleet --lib` | 36 passed (8 new) |
| `kontor-fleet-activation --lib` | 16 passed (2 new) |
| `kontor-daemon --lib` | 153 passed: 4 new formal tests, plus the 4 existing generic Committee tests moved onto a non-formal copy of the preset |
| `kontor-daemon --test loopback_api` (full, once, for impact) | 496 passed, 1 failed (`a_seeded_committee_runs_and_settles_instead_of_returning_503`, the boundary below), 1 ignored. The ignore is pre-existing: superseded by the kontor-jira connector tests. |
| `kontor-cli --test local_resolve` | 5 passed |
| `cargo clippy -p kontor-fleet -p kontor-fleet-activation -p kontor-daemon --all-targets -- -D warnings` | clean |
| `cargo fmt -p` (same three) `-- --check` | clean |

**Real 95037ece proof** (`/tmp/asma-8280-committee-cap-95037-proof-462-9YepzU4f`, `SHA256SUMS` sealed):
- **Input:** root `610e6955:config/orchestration/fleet.yml`, blob `ef6aae01`, sha `95037ece…6972`.
- **Method:** the actual `FleetSnapshot::activated` parser, both adapters and the one allocator. The harness test was appended once to the daemon test module, then removed; `applications.rs` returned to its pre-proof sha.
- **As written:** reviewer A `codex-work` gpt-6.1-sol, rung 1, openai; reviewer B `claude-work` claude-opus-5, rung 1, anthropic; judge `codex-work` gpt-6.1-sol, rung 2, openai. The judge's four rung-1 Claude routes are `vendor_cap_reached`, naming reviewer-b.
- **Variants:** with the judge's Sol accounts unavailable, or openai excluded, the result is `vendor_cap_exceeded` with no slot selected.
- **Generic reading:** the same routes seat two Claude seats.
- The two modes are identical in every case.

**5f45 compatibility** (`/tmp/asma-8280-committee-cap-5f45-compat-462-G9o0lcb4`; nothing merged):
- **Base:** a `git archive` of `5f45a561` (archive sha `ff7434c6…`).
- **Patch:** sha `a0ded748…`. Every hunk applies with offsets only, except one: the end-of-file test append in `kontor-fleet-activation/src/tests.rs`, where 5f45 has extra planning-pair tests. That block was appended verbatim and is byte-identical to the final file.
- **Results:**
  - `kontor-fleet` 36 passed;
  - `kontor-fleet-activation` 20 passed, the 4 planning-pair tests included;
  - `kontor-cli --test local_resolve` 10 passed, its 4 planning-pair tests included;
  - `kontor-daemon --lib` 157 passed.
- Scoped `loopback_api` (`committee fleet advisor consultation planning`): 78 passed and 1 failed. The failure is the same boundary test with the same rule, and every planning-pair loopback test passes.

## Mutation pass (`mutation-testing` skill; `/tmp/asma-8280-committee-cap-mutants-462`, `ledger.jsonl`, `SHA256SUMS`)

- **Baseline:** green at the final hashes (36, 16, 9 and 1 targeted).
- **Method:** one mutant at a time on the narrowest owning suite, then an exact restore with a fresh mtime and a hash check.
- **Score:** 23 seeded, 23 killed, 0 survived, 0 equivalent, 0 unviable. The checkout matched `final.sha` at the end.

| Mutant | File:line | Suite | Result |
| --- | --- | --- | --- |
| K01 judge not counted | `kontor-fleet/src/allocation.rs:437` | `kontor-fleet --lib` | KILLED (4, incl. `the_formal_review_seats_one_claude_and_moves_the_judge_to_rung_two`) |
| K02 cap `>=` → `>` | `allocation.rs:465` | same | KILLED (4) |
| K03 cap not released on backtrack | `allocation.rs:480` | same | KILLED (`the_cap_is_searched_jointly_not_greedily`) |
| K04 unknown maker fills a capped slot | `allocation.rs:393` | same | KILLED (`an_unknown_maker_cannot_evade_the_cap`) |
| K05 verdict rung cutoff removed | `allocation.rs:389` | same | KILLED (3) |
| K06 unstated rung admitted | `allocation.rs:386` | same | KILLED (`a_governed_slot_cannot_take_a_route_whose_rung_is_unstated`) |
| K07 cap failure classified as diversity | `allocation.rs:304` | same | KILLED (2) |
| K08 cap receipt names no holder | `allocation.rs:538` | same | KILLED (2) |
| K09 judge held to reviewer diversity | `allocation.rs:263` | same | KILLED (7) |
| K10 formal protocol never detected | `allocation.rs:594` | same | KILLED (7) |
| K11 cap allows two Claude | `allocation.rs:574` | same | KILLED (5) |
| A01 Local adapter omits constraints | `kontor-fleet-activation/src/lib.rs:783` | `kontor-fleet-activation --lib` | KILLED (2) |
| A02 Local holds only formal-bound slots | `lib.rs:780` | same | KILLED (`the_formal_review_seats_one_claude_and_moves_the_judge_to_sol`) |
| D01 governed adapter omits constraints | `kontor-daemon/src/applications.rs:15274` | `kontor-daemon --lib formal committee` | KILLED (4) |
| D02 recovery rung placed after declared rungs | `applications.rs:15384` | same | KILLED (`a_formal_recovery_route_keeps_its_own_ordinal_after_the_template`) |
| D03 recovery relabels a bound chain | `applications.rs:15379` | same | KILLED (`a_formal_recovery_route_cannot_relabel_or_broaden_a_bound_chain`) |
| D04 chain place unchecked | `applications.rs:15369` | same | KILLED (same) |
| D05 maker from provider family | `applications.rs:15372` | same | KILLED (`an_unbound_formal_committee_names_makers_only_from_the_selected_policy`) |
| D06 account expansion counted as a rung | `applications.rs:15384` | same | KILLED (`…keeps_its_own_ordinal_after_the_template`) |
| D07 unbound slot accepts chain provenance | `applications.rs:15380` | same | KILLED (`…cannot_relabel_or_broaden_a_bound_chain`) |
| D08 unknown source accepted | `applications.rs:15380` | same | KILLED (same) |
| D09 missing rank accepted | `applications.rs:15363` | same | KILLED (same) |
| D10 cap refusal loses its rule | `applications.rs:12860` | `loopback_api without_a_compliant_fleet…` | KILLED |

## Callback boundary (exact)

`a_seeded_committee_runs_and_settles_instead_of_returning_503` contains the ASMA-8090 legacy-row step (`a5e9983b`). That step:
1. invokes a fresh formal Committee;
2. strips the row's `admission` block with SQL to reproduce a deployed pre-provenance row;
3. proves that recovery accepts only template-declared routes.

For a row without admission, `committee_seat_route_provenance` accepts only a route the pinned template declares for that exact slot. So the step needs both of these to be template-declared:
- reviewer A, which it recovers;
- the judge, which it materializes when the second finding is recorded.

The bundled template declares only Claude for both. A Committee whose reviewer A and judge are both template-declared therefore seats two Claude seats, and a compliant fresh invocation cannot produce it. History agrees: per-slot admission provenance landed 2026-08-27 (`388633df`) and Committee fleet routing on 2026-09-25 (`7ffd650d`), so a genuine legacy row never held a fleet-routed seat.

Under the compliant fixture, the judge is fleet-routed. Recording reviewer B's finding is then refused with `the active Committee route has no immutable template or reroute provenance`. That is correct behaviour for a row that never existed historically.

Every resolution I can see is one of the following, and each needs the LSA's decision:
- a selected-definition change;
- rewriting a seat's stored route in the fixture (history rewrite);
- seeding a pre-cap two-Claude row directly;
- narrowing the legacy step.

No such change was made.

## Limits

- All eligibility in these tests is fake, from scripted ports. No provider, account, native or quota qualification is claimed.
- Post-admission single-seat paths keep their own single-seat checks and do not count the formal cap: Committee seat recovery (`/seats/{binding}/recover`) and the native-less reroute. A successor could seat a second Claude. This is out of this correction's scope and is recorded for the LSA.
- A formal Committee with no selected policy, or with routes the policy does not list, is now refused (ruling C). Operators need a selected policy that names the routes' makers.
- The no-maker refusal reuses the existing slot rule text. Only the cap refusal has a new rule.
- Disclosure: one earlier daemon check this turn used artifact `431a7`'s `cargo-home` and rewrote its unhashed `.global-cache`. That file is not in its `SHA256SUMS`, which still verifies. Every later build used a private clone. Artifact `431a7`'s 32 canonical hashes and its source acceptance are unchanged.
- Reporting note on artifact `431a7`, which stays immutable: it has 6 bindings, not 7. `summary.roster_seats` read null because of a wrong top-level path. The canonical roster has exactly 2 seats.

## Successor source phase — 2026-10-03

This section supersedes the predecessor's uncommitted status, callback boundary, and post-admission scope exclusion above. The earlier sections and all earlier failure/mutation artifacts are preserved as historical producer evidence.

- Logical seat: existing TASK002 implement successor to retired `462`, parent TPM47; supervisor attested actual parent/workspace/project/cwd before dispatch. Session `01a1005b-67f1-7410-8aea-2655fa29d33d`, Personal Sol 6.1, effective xhigh, full access, Plan false.
- Source authorship is **MIXED**: Anthropic/Claude implementer `462cd93d-30ec-4ed0-9841-3687612da5e0` authored the inherited WIP; OpenAI implementer `72e7bfa4-b73e-4358-91b8-996e6f9a0598` continued the same TASK002 seat as `codex-personal` / `gpt-6.1-sol` at xhigh. Predecessor 462 was closed and archived at `2026-10-03T06:02:31.775Z` before takeover. The source commit carries both authorship phases; the OpenAI successor identity does not replace the Anthropic WIP provenance.
- Base/required parent: `545b44a1a693b3cc054bf453709a1dc4149b7440`; base tree `fb0f9c871868cf7574e91c69029b1f111a2fdeb5`. Existing ASMA-8280 branch and ancestor chain retained.
- Before writes, all nine manifest hashes in `/tmp/asma8278-takeover-20261003-f0umjyqa/MANIFEST.json` matched the inherited eight tracked WIP files and untracked handoff; index was empty. `BRIEF.md`, `WIP.diff`, and the copied handoff were read. The authorized predecessor WIP was carried forward; unrelated writers' WIP was not incorporated or changed.
- Current evidence is **SOURCE_VALIDATED_FIXTURE**, produced by this implementation seat. It is not independent verification, native/account qualification, TASK002 completion, or ASMA-8280 completion. TPM alone owns the requested 2399 -> e5 review dispatch. No self-dispatch occurred.

### Implementation and the exact fixture correction

Recovery, native-less reroute, findings admission, and deferred materialization now invoke the same `kontor_fleet::allocate_constrained` evaluator used by both allocation modes. The whole formal snapshot includes the deferred Judge; unaffected seats remain fixed. Only ordered proposals for the addressed logical slot vary. The constraints retain all-three-seat Anthropic max 1, distinct vendors for reviewers only, and formal source rungs 1–2. Unknown rank or vendor refuses. Bound-chain step/sub-step comes from an exact chain rung; changing preference order cannot promote R3 or count account expansion as a new rung. Account registration, quota qualification, source-policy parsing, route validation, current generation, authorization, and existing replay fences remain in place.

The store's existing current-generation recovery/reroute surfaces provide an immutable canonical profile/hash and unresolved state. Formal validation reads run revision, every seat, and those profiles in one store snapshot. Preparation's existing transaction CAS advances run revision and target generation before native retirement; two simultaneous proposals cannot both reserve the shared budget. A peer unresolved reservation refuses before attempts, receipts, retire/archive, persisted reroute, or native launch. A same-route pending credential recovery may still admit an incomplete peer reviewer finding; it cannot authorize deferred Judge launch. Native-less reroute uses the existing native lifecycle write guard to drain materialization/recovery before testing native-less state; no schema, fence, or trust protocol was added.

New accepted recovery profiles freeze the allocator-selected constraint candidate and selected policy hash inside the existing hashed profile. Old durable native-less crash replay profiles retain their exact accepted `ordered_routes` and canonical hash. Those routes keep their accepted source ordinal; unproven ranks remain ineligible. Exact completed durable replay remains authorization-first and causes no additional native effect. Healthy peer identity, model, effort, member generation, immutable findings, results, and the pinned definition are not rewritten. Generic non-formal behavior is preserved.

The modern broad `a_seeded_committee_runs_and_settles_instead_of_returning_503` retains authentic admission and its original topic, uniqueness, permissions, generation fencing, interrupted recovery/replay, findings, Judge gate, settlement, results, and completion assertions. Its fixture now explicitly registers/names a compliant Cursor Judge R1 -> Sol Judge R2 route. The prior newly Sol Judge could not lawfully perform the later provider-unavailable Sol recovery on its bound chain; the explicit fixture supplies the distinct route the original assertion requires.

The SQL-stripped admission/fleet-only Judge hybrid is now a separate **simulated provenance-loss negative** regression. It removes only admission in its isolated test row, restores the exact frozen-input trigger, verifies the trigger, retains template/hash/semantic identity/seats, and expects `placement_blocked` at Reviewer A recovery and finding admission. The exact boundary is `the active Committee route has no immutable template or reroute provenance`. No attempt, receipt, finding, result, invented provenance, repair, or unauthorized Judge/native effect is created. The immutable `01991c00-0000-7000-8000-000000000001@1` two-Claude default is unchanged and remains refused when unbound. Genuine legacy template-declared/stored-generation/crash-replay fixtures remain in the suite without relaxed assertions or new SQL authority.

### Fresh validation

Every Cargo invocation used toolchain 1.97.1, `--offline --locked -j 2`, `env -i`, a fresh private HOME/TMPDIR and private cloned CARGO_HOME/target, with no sccache. Live providers, credentials, native runtimes, databases, and daemon state were not used; integration state and native calls are isolated fake fixtures.

| Check | Fresh result |
| --- | --- |
| Fleet library | 36 passed |
| Activation library | 16 passed |
| Daemon library | 154 passed |
| Store library | 16 passed |
| Full loopback | 503 passed, 0 failed, 1 pre-existing ignored |
| CLI local resolve | 5 passed |
| Clippy, fleet/activation/daemon/store, all targets, `-D warnings` | clean |
| Rustfmt, all nine changed Rust files, `--check` | clean |
| Restored mutation targets | fleet 36 / activation 16 / daemon formal-Committee 10 / loopback 9 passed |
| Actual 95037 Rust parser and both modes | 1 passed; Sol A R1 / Opus B R1 / Sol Judge R2, deterministic backtracking, blocked exclusions, generic behavior preserved |

The real proof input SHA-256 remains `95037ece70fb6aa59d5760f2313b7b16717c9a619b389db561ed706f1b586972`. Fresh proof receipt records harness, log, seeded and restored source hashes. The temporary harness was removed byte-for-byte; the source restored to `54d4bd0888f6d2b2253600603994e3f72181a88ac7958aaa113efcdc2bbf5056`. No provider/account/native qualification is claimed.

All successor intermediate failures are retained in the private logs, including the initial full `502 passed / 1 failed / 1 ignored` crash-profile boundary, incorrect source-substep test expectation, lint helper argument count, and earlier fixture/debug runs. The final full pass supersedes those source/fixture failures without deleting their receipts. Historical `496 passed / 1 failed / 1 ignored`, old 5f45 compatibility failure, and predecessor 23-mutant claims remain historical and were not recast as fresh or independent results. Historical seals rechecked read-only: 35 mutant-artifact entries, 10 old proof entries, 15 compatibility entries, and 8 compatibility final-source entries all match.

### Provenance, disclosure, and remaining review boundaries

Artifact `431a7` remains historical **SOURCE_VALIDATED_FIXTURE** with 32 indexed canonical artifacts, six high bindings and two canonical roster seats. The earlier unhashed registry `.global-cache` write and supervisor-disclosed prior-462 Claude provider-home memory side effect remain disclosed. This successor did not edit provider homes or memory; that statement does not retroactively erase earlier effects. No global policy, chain, pin, activation, provider, credentials, native runtime, live DB, daemon, root/consolidation/ECP/foreign/R3 state was mutated by this source phase.

This is a local source checkpoint only. No push, pull, merge, reset, stash, Jira/completion action, deployment, live qualification, or independent review occurred. No new schema or concurrency protocol was introduced. Broader release/online verification gates and fresh application of this successor delta to 5f45 are not claimed; the existing historical compatibility artifact is preserved. No incompatible original assertion remains in the current isolated suites. Review 2399 -> e5 and task/epic acceptance remain with TPM.

### Fresh mutations and source receipts

The producer ledger records 17 executions across 16 unique seeded source hashes, with 17 KILLED and 17 exact restorations and no invalid builds or survivors. S02 and S17 repeat the same unknown-maker seed (`3ede777c02636fde13ef848df497f628ad2ad3aa638bf91df072041032d06130`) in two suites: fleet library and daemon recovery API. Seeding and execution belong to the producer; 2399 later inspected the ledger without independently reseeding it. The predecessor's 23 historical rows are not recounted in these totals. Each execution ran one seeded change at a time; baseline, mutated, restored, and log SHA-256 values remain in `mutants/ledger.json`. Source status matched the baseline after restoration.

| Mutant | Changed decision | Result |
| --- | --- | --- |
| S01 | Judge omitted from cap | KILLED; exact restoration |
| S02 | Unknown maker admitted | KILLED; exact restoration |
| S03 | Unstated rank admitted | KILLED; exact restoration |
| S04 | R3 verdict admitted | KILLED; exact restoration |
| S05 | Local constraints omitted | KILLED; exact restoration |
| S06 | One Local rebind relaxes cap | KILLED; exact restoration |
| S07 | Formal constraint detection discarded | KILLED; exact restoration |
| S08 | Recovery whole Committee cap omitted | KILLED; exact restoration |
| S09 | Pending peer treated terminal | KILLED; exact restoration |
| S10 | Finding stored before provenance guard | KILLED; exact restoration |
| S11 | Durable replay claims new effect | KILLED; exact restoration |
| S12 | Preference order promotes bound R3 | KILLED; exact restoration |
| S13 | Store hides pending reservations | KILLED; exact restoration |
| S14 | Store classifies pending as installed | KILLED; exact restoration |
| S15 | Store reads predecessor generation | KILLED; exact restoration |
| S16 | Exact recovery replay launches the deferred Judge again | KILLED; exact restoration |
| S17 | Unknown maker passes recovery before native retirement | KILLED; exact restoration |

S09 and S14 are KILLED because the second frozen-candidate guard still refuses with a different rule from the expected pending-reservation refusal. These two records are not unique proof of the pending guard. S13 and S15 hide the pending reservation and actually allow a peer native recovery (200 instead of 409), establishing the need for the store query. S08 permits a second Anthropic recovery; S10 persists a finding/receipt before provenance validation; S16 launches a deferred Judge on replay; S17 permits an unknown maker recovery. Those effects are all isolated scripted fixture effects.

Fresh artifact directory: `/var/folders/t3/0tbx772d571_57t01yb9twx00000gn/T/asma8280-task002-successor-uxneic5q`. `INDEX.md`, `SHA256SUMS`, `source-final-sha256.json`, `review-receipt.json`, and the final ASMA dry-run/commit logs provide the exact resulting source, HEAD/tree/parent, archive, tests, and restoration receipts.

- Fresh mutant ledger SHA-256: `deb211050cee58ddb03291c8593a75ef317175f02b5bc7eca81894bb67d860ac`.
- Fresh 95037 proof receipt SHA-256: `4f1276655bfde03d12f95dccfa48009a0c74184f9a709c0e80d752d847e95583`.
- Historical artifact recheck receipt SHA-256: `3f674f20a15b1e0e2c6a1874088a346b305cf9645fb4724ef941449258f78d47`.

| Final code path | SHA-256 |
| --- | --- |
| `crates/kontor-daemon/src/applications.rs` | `54d4bd0888f6d2b2253600603994e3f72181a88ac7958aaa113efcdc2bbf5056` |
| `crates/kontor-daemon/tests/loopback/committee_evidence.rs` | `dd31429201fc3e4f8c346700804bf3114f44a11ebe6cb9d957847f6dd855a261` |
| `crates/kontor-daemon/tests/loopback_api.rs` | `c0c71c9935bcfd5c4f019c148d7dcd7cefeaa062a1bb3e0de784ed1838238621` |
| `crates/kontor-fleet-activation/src/lib.rs` | `f412abb489c887f6abb984bd7dc77775039fdc679c4cc24a29ddc6de3285783e` |
| `crates/kontor-fleet-activation/src/tests.rs` | `6c52e07732bbc7177f846d110f0fff361183a978b92d549a9faccb9578a5f65f` |
| `crates/kontor-fleet/src/allocation.rs` | `3b964b061e4aede88adc5be23f4ecce3d552e4663e796f2f8efbab5a43ed8ef7` |
| `crates/kontor-fleet/src/lib.rs` | `2c3e5d4170e152e65ea0b2ba7ecb675d8d4967982d3b84c0b2d320e25e66ca5c` |
| `crates/kontor-fleet/src/tests.rs` | `7d852e0288bb989fc69383e489ba953c31934ed8447a3fcc16aa2314997ba7e5` |
| `crates/kontor-store/src/repository.rs` | `fb8b9ffe0826e621ff82d023454f70dc457fe90f2e6a4c13fa2b9cd62b5c2021` |

The handoff itself is included separately in `source-final-sha256.json` to avoid a self-referential hash. The local commit identity is in `review-receipt.json`; no commit hash is guessed here.

## Handoff-only correction — 2026-10-03

TPM admitted this documentation rework to the same implement seat 72e7 after the focused instruction-impact addendum. Only this handoff changes. Source candidate `0d9e21a5a751977a5ce5b462636000c3ebd6d1ea`, tree `0ea0056db1392c4fd5e6278cf28ddbb2a5be1659`, prior verdicts, producer logs, ledgers, hashes, and acceptance history remain immutable. No Cargo, tests, mutant execution, quota/provider probe, or native qualification was repeated for this correction.

### Evidence attribution and current instruction scope

| Evidence | Scope retained |
| --- | --- |
| Anthropic predecessor 462 | Historical 496 passed / 1 failed / 1 ignored and old 5f45 same failure; 23 historical mutation rows, not recounted |
| OpenAI successor 72e7 | Producer full loopback 503 passed / 0 failed / 1 ignored; source-turn libraries 222, CLI 5, real 95037 Rust proof, lint/format, and 17 ledger executions / 16 unique seeds |
| Independent verifier 2399 | Source verification PASS: independently ran libraries 222, scoped loopback impact 9, CLI 5, lint/format, and the actual 95037 Rust proof. Full producer 503 was not independently repeated; mutation ledger was inspected, not independently reseeded |
| Focused instruction-impact addendum | PASS with no fatal gaps for current adapter restoration and applicability of existing source evidence to matching selected e18818b9 requirements; observations reused without test/mutant reruns |

The immutable independent source verdict is `/tmp/asma-8280-task002-verify-4447678e/out/independent-verdict.json`, SHA-256 `01de9c145edcdbdfd42618d2df0044d487e3dafd3f67d6ffad5bfc74d2a65a83`, classification **SOURCE_VALIDATED_FIXTURE**. The focused addendum is `/tmp/asma-8280-task002-addendum-4447678e/instruction-impact-addendum.json`, SHA-256 `5e8973bf820c5c0f93db6a48776a79ca8aaa0942257d07e200b685d2330643fc`. Neither is task/epic/adoption/runtime acceptance or formal Committee/AUD PASS. This correction changes documentation provenance only and fabricates no code/test results, hash acceptance, or downstream approval.

Current instructions select root commit `e18818b9d8d29c9b6003ffae21d2d1ede0bde356`, snapshot `/tmp/asma8280-instruction-snapshot-mv08vqyt/snapshot`, manifest SHA-256 `58a50ae2314f8fce148a0ef3b3b76230ad127acf302833cbda42b46fa7b10f19`. The selected installer `_tools/scripts/check-codex-agent-config.py` has git blob `216e8843c3fff712a3b44ec8221b600d9d050b46`. Current restoration installed 166 adapter entries and its explicit workspace/target `--check` passes. The original source-turn canonical instruction revision remains **UNKNOWN**; the prior native adapter check was **UNMET** history (162 MISSING entries), not a retroactive pass or a verified old/new instruction diff. Current restoration does not establish historical prelaunch compliance, automatic loading, or closed-tools qualification. Quotas remain **UNMEASURED**; later live qualification remains **UNMET**, rather than a claimed failed Cursor admission.

The producer's 129-entry `SHA256SUMS` covers curated evidence, not mutable Cargo/cache/target/home/temp state. The earlier unhashed `.global-cache` write and prior-462 Claude provider-home memory side effect remain disclosed, as do artifact 431a7's 32 indexed SOURCE_VALIDATED_FIXTURE artifacts, six high bindings, and two roster seats. No old receipt or seal is rewritten to include this later handoff revision.

Claude audit occupant `00c73384-a09c-4b22-b2dd-0a5331bf64cc` acknowledged the brief and had an autoloaded memory read; that ACK is neither formal review nor independent PASS. Mixed Anthropic/OpenAI source authorship must remain visible when audit eligibility is decided. The third-vendor audit proposal is pending the actual root decision; this correction claims no new approval, admission, or worker generation. Historical 2399 -> e5 references above remain chronology. TPM owns focused documentation review by the same 2399 seat, then any eligible audit dispatch and subsequent acceptance. No worker was started or self-dispatched.

Before this write, native readback confirmed the same 72e7 / Personal Sol 6.1 xhigh / full-access / Plan false session `01a1005b-67f1-7410-8aea-2655fa29d33d`, parent `47cb9a74-a798-4f40-9b54-2a818afe0a42`, workspace `wks_a284b5d96a0ea32b`, project `prj_e9f8052597f78919`, and receiving cwd `/Users/igor/carasent/asma-modules/.worktrees/asma-8280/asma-rs-kontor`, with no pending permissions. TPM's pre-dispatch readback was idle; this correction's readback is the active admitted turn. Current native session/context capacity was read without a provider or quota probe. HEAD/tree matched the source candidate, the index was empty, tracked files were clean, and the five untracked adapter paths were preserved. The selected seat-handoff and commit procedure revisions come from the e18818b9 snapshot; the owning plan remains `.asma/workspace/_docs/ai-orchestration/plans/2026-09-27-20-14-plan-shared-orchestration-workflow.md` on the existing Paseo-direct TASK002 scope.
