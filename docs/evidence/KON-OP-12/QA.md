# KON-OP-12 / ASMA-7881 — QA

Date: 2026-08-19
Branch: `feat/ASMA-7881-kontor-ambiguity-disposition` (`_tools/asma-rs-kontor`)
Baseline: `origin/master` `615d24b`

## Commands from the handoff's `verification.commands`

Run as a script that records each command's real exit code, with full output kept
in one log and the failure lines grepped from the log afterwards — not from a
truncated pipeline. Every command exited `0`.

| Command | Binaries | Passed | Failed |
| --- | --- | --- | --- |
| `cargo fmt --all -- --check` | — | clean | — |
| `cargo clippy --workspace --all-targets -- -D warnings` | — | clean | — |
| `cargo test -p kontor-core` | 7 | 167 | 0 |
| `cargo test -p kontor-policy` | 5 | 39 | 0 |
| `cargo test -p kontor-scheduler` | 5 | 47 | 0 |
| `cargo test -p kontor-store` | 21 | 315 | 0 |
| `cargo test -p kontor-daemon` | 6 | 207 | 0 |
| `cargo test --workspace` | 117 | 1547 | 0 |

Beyond the handoff's list, because this change moved a generated contract:

| Command | Result |
| --- | --- |
| `KONTOR_UPDATE_CONTRACT=1 cargo test -p kontor-api --test openapi_contract` | regenerated `contract/openapi.json` (+50 lines, additive) |
| `pnpm --filter kontor-console generate:api` | regenerated `apps/console/src/api/schema.d.ts` (+14 lines, additive) |
| `pnpm --filter kontor-console verify:api` | committed types match the served contract |
| `pnpm --filter kontor-console typecheck` | clean |

The two new `CompletionBlockerDto` variants are the only schema delta. Both are
additive `oneOf` members; no existing field, route or DTO changed. `openapi.json`
and `schema.d.ts` are pinned by `kontor-api`'s own contract suite, so leaving
either stale would have failed the build rather than drifted silently.

## Three schema pins failed first, and my earlier green report was wrong

Migration `0041` broke three `kontor-store/tests/schema_v1.rs` assertions, and I
initially reported the store suite as green when it was not. The cause was my
verification method, not a flake: I ran `cargo test | grep … | head -N`, so the
shell reported the exit code of `head` rather than of `cargo`, and a later
workspace run had its output truncated by `head` before the failing lines. Both
"the store suite passed" and a "1042 passed, 0 failures" figure I quoted were
therefore unfounded. The rerun above avoids pipelines entirely.

Two of the three were ordinary pins that a new migration is expected to move:

- `an_empty_database_migrates_to_the_current_schema_version` — `SCHEMA_VERSION`
  40 → 41;
- `the_schema_contains_exactly_the_expected_tables_and_they_are_all_strict` — the
  four new tables added to the expected set.

**The third was a real defect in shipping code.**
`the_operational_hardening_v35_lineage_converges_without_losing_its_receipt`
failed because `migrations.rs` converges that historical lineage through an
explicit migration list rather than `MIGRATIONS[pending..]`, and the list ended at
`MIGRATIONS[39]`. Any realm on that lineage would have converged to version 40 and
then failed to open. Fixed by adding `MIGRATIONS[40]`. A fresh-database test
cannot see this; only the historical-shape test can.

## New suites

| Suite | Tests | Covers |
| --- | --- | --- |
| `kontor-core/tests/open_question.rs` | 27 | Disposition truth table, exactly-three parse/serde, deferral needs a concrete trigger, trigger match/mismatch/double-fire, corrections append byte-identically, any seat raises, closer split governs closing, `project_shared` default and its immutability, and the five detector cases. |
| `kontor-scheduler/tests/completion.rs` | +6 (10 total) | Undispositioned and reopened questions each hold the epic in `closeout` with a typed blocker naming id and subject; every disposition releases; a question raised *after* completion started still blocks; one blocker among many suffices; closeout receipts and questions report as independent gates. |
| `kontor-store/tests/open_questions.rs` | 12 | Migration inventory, child `UPDATE`/`DELETE` refusal, header-immutable-except-revision, deferral/firing schema rules, cross-project non-resolution, stale-revision refusal, current-deferral-only reopening, restart round trip, deterministic export, import lineage, snapshot restore. |
| `kontor-profiles/tests/operational_domain.rs` | +2 (10 total) | Both closer codes exist in the pinned catalog and differ; an undeclared closer is refused. |

## Mutation run

Each defect from the handoff's `mutation_targets` was seeded into the source,
the relevant suite run, and the source restored.

| Seeded defect | Outcome |
| --- | --- |
| `MarkDone` ignores open-question blockers | **caught** — 4 scheduler tests red |
| Deferral accepted with no concrete trigger | **caught** — `deferring_requires_a_concrete_non_empty_trigger` |
| `status()` ignores a matching fired trigger | **caught** — 2 core tests red |
| Any role may close any question | **caught** — 2 core tests red |
| Schema trigger `open_question_firing_matches_its_deferral` dropped | **caught** — `a_firing_that_names_the_wrong_trigger_is_refused_by_the_schema` |

### One mutant initially survived, and the test was wrong

Dropping the firing-matches-deferral trigger first left the store suite **green**.
The probe inserted a firing against disposition ordinal 1 on a question that had
no dispositions at all, so the *foreign key* refused the row and the trigger was
never exercised — a test that looked like it covered the rule and did not.

Fixed by isolating the rule: the question now carries a real deferral on one
trigger and the probe names a different one, so only the trigger can refuse it,
plus a positive case inserting the deferral's own trigger to prove the refusal
was the rule and not a blanket rejection. The probe connection also now sets
`PRAGMA foreign_keys = ON`, since foreign keys are per-connection in SQLite and a
raw probe would otherwise accept rows the daemon's connection refuses. The mutant
is caught on re-run.

Two targets are not mutation-testable by design, and that is stronger than a test:

- **detector writes or changes aggregate state** — `DetectorObservations` holds
  only shared borrows and has no repository or command port, so a mutation that
  wrote from `detect` does not compile. A byte-identical assertion across
  `detect` covers the observable half.
- **new escalation / notification / auditor-role surface** — nothing was added,
  so there is nothing to mutate; verified by inspection of the diff and by grep
  over `kontor-mcp` and `kontor-cli`, which contain no open-question symbol.
