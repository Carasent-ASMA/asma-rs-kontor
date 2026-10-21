---
name: mutation-testing
description: "Check whether tests detect seeded defects, especially when requested or required by the owning plan. Not routine test generation, coverage measurement, or an automatic step for every small code fix."
---

# Mutation testing (test the tests)

Coverage proves a line ran. Mutation testing proves the line is checked. The
loop: break the code on purpose, rerun the suite and read the result backwards.
A mutant that survives — the suite stays green — means no test asserted that
behavior, so the test does not do what its name claims.

This skill is the manual or agent-driven form: no tooling required, works in
every project, minutes per pass. Automated mutation runners are a later option
(see [Automation](#automation-not-installed-today)).

## When it is mandatory

- **End of every implementation-plan phase** that changed runtime behavior —
  see [`create-implementation-plan`](../create-implementation-plan/SKILL.md)
  (`MUT-` entries in the plan's Testing section).
- **Before a plan is marked Completed** — a plan with unresolved SURVIVED
  mutants is not done.
- **Before deleting or replacing an existing suite** — prove the replacement
  kills what the old one killed.
- **After a bug escapes** in an area the suite already "covers" — the escaped
  bug is the mutant; start there.
- **On demand** — when the operator requests it.

Skip for type-only changes, generated code, configuration, documentation and
pure moves or renames.

## Scope — mutate the diff, not the repository

Candidates are the changed source files (`git diff --name-only <base>`),
intersected with real logic. Rank by risk: guards and permission checks,
writes and persistence, money/time/ID arithmetic, query filters, retry and
cache invalidation. Three to five mutants per changed file, timeboxed; this is
a spot check, not a proof.

Never mutate generated output, migrations, fixtures or vendored code.

## Mutant catalogue

Pick the mutants that match the code in front of you. The first three catch most
vacuous tests.

| # | Mutant | Example | Catches |
|---|--------|---------|---------|
| M1 | Skip the write | comment out the `INSERT`/`UPDATE`/`save()` | test asserts on its own fixture, not on what the code persisted |
| M2 | Hardcode the return | `return computed` → `return []` / `return true` / `return null` | test only checks that the call did not throw |
| M3 | Delete a guard | drop an early return, `if (!x) throw`, or a permission check | error and authorization paths never exercised |
| M4 | Flip a comparison | `>` → `>=`, `===` → `!==`, `&&` → `\|\|` | boundary conditions untested |
| M5 | Drop a filter | remove a `where` clause, `.filter()` or tenant scoping | test data too thin to notice extra rows |
| M6 | Drop an ordering | remove `ORDER BY` or `.sort()` | ordering asserted by insertion luck |
| M7 | Swap adjacent arguments | two same-typed parameters in a call | positional bugs; fixtures where both values are equal |
| M8 | Off-by-one | `i < n` → `i <= n`, `slice(1)` → `slice(0)` | pagination, versioning, ranges |
| M9 | Negate the invalidation | skip a cache bust, event emit or `revalidate` | a side effect asserted nowhere |

## Procedure

Run one mutant at a time. Never batch.

0. **Baseline** — run the target suite; it must be green. A red baseline makes
   every later result meaningless. Record `git status --porcelain` for the files
   you will touch; the tree is often already dirty, so you need to know what was
   yours before the mutant.
1. **Snapshot the file** — `cp <file> <file>.mut.bak` if it had uncommitted
   changes; otherwise `git checkout -- <file>` is enough to revert later.
2. **Seed exactly one mutant.**
3. **Run the narrowest suite** that claims to cover it.
4. **Record** KILLED (which test failed, on which assertion) or SURVIVED.
5. **Revert immediately** — restore from the snapshot or `git checkout --
   <file>`. Verify with `git diff -- <file>`; the mutant must be gone before the
   next one is seeded.
6. Repeat. At the end, `git status --porcelain` must match the baseline from
   step 0 exactly.

Hard rules:

- One mutant at a time; two at once makes a KILLED result unattributable.
- Never commit a mutant. Never leave one behind, even briefly on a shared
  branch.
- A mutant that makes the code fail to compile or typecheck is not a mutant; the
  type checker killed it. Pick another.
- Do not fix a survivor while other mutants are still pending. Finish the pass,
  then fix.

## Runners

Use the project's own runner and the narrowest invocation available. Examples:

```sh
pnpm vitest run path/to/file.test.ts          # one file
pnpm vitest run -t 'rejects a stale version'  # one test, for a slow suite
pytest path/to/test_file.py::test_name        # Python
```

- **Database-backed suites** need their environment up. A mutant that
  "survives" because the suite errored on connection is not a result; fix the
  environment and rerun.
- **Browser end-to-end suites** are too slow to mutate against, including
  behaviour scenarios written as `.feature` files. An end-to-end scenario
  carries no `MUT-` row and is marked `report-only` in the plan's scenario
  table. Mutate at the unit or integration layer. Changed source is mutated
  regardless of which scenario covers it; a scenario note never waives that.
- **Python projects** use the same loop with `pytest`.

## Verdict and follow-up

- **KILLED** — the test earns its name. Nothing to do.
- **SURVIVED** — a finding. Fix the assertion, not the symptom: strengthen the
  existing test so it pins the real behavior. Writing a brand-new test aimed at
  the mutant leaves the original vacuous test in place, still lying about what
  it covers.
- **EQUIVALENT** — the mutant changes no observable behavior (a redundant guard,
  a stable-sort tiebreak nobody can see). Not a finding; record one line of why
  and move on.

Score is `killed / (total - equivalent)`. Use it as a trend per plan, not as a
CI gate.

## Why mutants survive — read the survivor, then the test

- The test asserts on the fixture it seeded rather than on what the code
  returned or persisted (kills M1).
- The assertion is `toBeDefined()`, `not.toThrow()`, a bare status code or a
  snapshot.
- A mock stands in for exactly the path that was mutated.
- The test asserts a spy was called instead of asserting the resulting state.
- Shared setup makes the expected value coincide with the default or empty value
  the mutant returns.

## Reporting

For a plan phase, record the pass in the plan as `MUT-00x` rows. For an ad-hoc
run, reply with the same table.

| Mutant | File:line | Suite | Result | Action |
|--------|-----------|-------|--------|--------|
| M1 skip insert | `src/example/repository.ts:88` | `repository.test.ts` | SURVIVED | assert the row is readable back through `findById` |
| M4 `>=` → `>` | `src/example/versionGuard.ts:31` | `versionGuard.test.ts` | KILLED (`rejects equal version`) | — |

Record KILLED or EQUIVALENT only for a mutant that was seeded and run. A
`report-only` scenario row is not a MUT result.

## Automation (not installed today)

The pack does not require an automated mutation runner. If a project adopts one,
pilot it only after the manual pass has proven it finds things worth fixing, and
then on one small, fast package — not a database-backed suite, because the
runner reruns the suite per mutant. Keep it nightly and scoped to changed files;
do not gate pull requests on a mutation score.
