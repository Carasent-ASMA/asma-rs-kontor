# ASMA-8202 high-change record — AUD-8202-001

Date: 2026-09-17
Gate: high-audit-gate sequence 1, REJECTED, routed to the implement seat at
workflow revision 5. This record is proposed, not approved; no gate action is
taken here.

## The finding

The resident retry read unconfirmed admissions with
`ORDER BY event.decided_at, event.id LIMIT 16`, always from the oldest row.
Sixteen admissions that never recover are exactly one page, so they were
re-read on every tick and every admission behind them was never attempted at
all — for as long as those sixteen stayed eligible, which for a persistently
blocked admission is indefinitely.

The bound was not the fault. Always starting from the same end was.

## The change

`SqliteStore::unconfirmed_admissions` takes a keyset resume point and each
returned row carries its own position:

- `AdmissionScanKey` holds `decided_at` and the event `id` exactly as the row
  stored them, so the comparison is made against the stored bytes and cannot
  drift through a parse and re-format.
- the predicate is `decided_at > ?4 OR (decided_at = ?4 AND id > ?5)`, which
  matches the `ORDER BY` exactly. The tie-break is load-bearing: two admissions
  decided in the same instant would otherwise be separated by nothing, and the
  second would be skipped forever by a position that had already passed it.
- `after = None` reads from the oldest, unchanged.

The resident reconciler carries the position across ticks:

- each scan resumes after the last row the previous scan took, **recovered or
  refused** — a blocked admission that held its place would be retried forever
  ahead of the admissions it is blocking, which is the starvation itself;
- an empty read *behind a position* means the order is exhausted, so the scan
  wraps to the oldest and rotates again. An empty read from the start means
  there is genuinely nothing to do and must not wrap;
- the bound of 16 per scan is unchanged.

The two decisions are pure functions, `scan_reached_the_end` and
`scan_resume_point`, so they are reachable by a test rather than only by a
running realm.

### Why every eligible admission is eventually revisited

The scan order is total — `(decided_at, id)` with `id` a primary key — so the
eligible set has a well-defined sequence. Each scan consumes a non-empty prefix
of the sequence strictly after its position and then advances the position to
the last row consumed, so no scan can return the same row twice in a row, and
the position advances monotonically until the sequence is exhausted. Exhaustion
resets the position to the start. The eligible set is finite, so the rotation
period is bounded by `ceil(eligible / 16) + 1` scans, and every eligible
admission is read within one period whether or not any of them recovers.

## Regressions added

`a_blocked_first_page_cannot_hide_later_admissions` — twenty eligible
admissions, more than one page:

- the first page is the oldest sixteen and nothing else, which is the whole of
  what an uncursored scan could ever see;
- none of the later four appears in it — the starvation, asserted directly;
- resuming after that page reaches exactly those four, with all sixteen still
  eligible and untouched;
- past the last admission a cursored read is empty, which is the wrap signal;
- following the resident rule, all twenty are visited though none recovers, and
  the rotation then wraps back to the oldest page.

`admissions_sharing_a_decided_at_are_not_skipped_by_the_resume_point` — two
admissions decided in the same instant plus one later. Resuming after the first
of the tied pair must reach its sibling, whose timestamp is not greater. The
test learns the scan order rather than assuming it and asserts the pair really
does share an instant, so it cannot pass vacuously.

Three unit tests cover the rotation decisions directly:
`only_an_empty_read_behind_a_cursor_ends_the_rotation`,
`a_scan_resumes_after_the_last_admission_it_took`,
`a_scan_that_took_nothing_leaves_the_position_alone`.

## Mutations

Each was applied alone to production source, compiled, executed, and restored.

| ID | Mutation | Result | Killed by |
| --- | --- | --- | --- |
| M-F1 | drop the keyset predicate, restoring the audited behaviour | killed | `a_blocked_first_page_cannot_hide_later_admissions` |
| M-F2 | `decided_at >= ?4`, so the position's own row repeats | killed | `a_blocked_first_page_cannot_hide_later_admissions` |
| M-F3′ | defeat the tie-break while keeping the parameter bound | killed | `admissions_sharing_a_decided_at_are_not_skipped_by_the_resume_point` |
| M-F4 | never wrap: the rotation stops at the end of the order | killed | `only_an_empty_read_behind_a_cursor_ends_the_rotation` |
| M-F5 | wrap on any empty read, ignoring the position | killed | `only_an_empty_read_behind_a_cursor_ends_the_rotation` |
| M-F6 | resume after the first row scanned instead of the last | killed | `a_scan_resumes_after_the_last_admission_it_took` |
| M-F7 | an empty scan discards the position it was given | killed | `a_scan_that_took_nothing_leaves_the_position_alone` |

Three corrections to this seat's own mutation testing, recorded because each
would otherwise have left a false result standing:

1. The first attempt at M-F3 removed the tie-break clause outright, which also
   removed the only reference to `?5`. It failed with
   `Wrong number of parameters passed to query. Got 5, needed 4` — an arity
   error, not a fairness failure. **That kill was false**, and it exposed a real
   gap: the twenty-candidate regression gives every admission a distinct
   instant, so the tie-break never mattered to it. M-F3′ preserves arity and is
   killed on behaviour by the new shared-instant regression, which the
   twenty-candidate regression does **not** catch.
2. M-F4 and M-F5 first appeared to survive. The filter used was `scan`, which
   does not match `only_an_empty_read_behind_a_cursor_ends_the_rotation` — the
   test that kills them never ran. Both are killed when the suite is run.
3. A `git checkout --` used to restore a mutation reverted the implementation
   itself, which was uncommitted at that moment. It was re-applied and the
   change is now committed before any mutation is seeded.

## Suites, with baseline separation

Candidate `a4274480`; base `1782c740`, the same branch immediately before the
remediation commits.

| Suite | Candidate |
| --- | --- |
| `kontor-store --test scheduler_admission` | 34 passed, 0 failed |
| `kontor-daemon --lib` | 84 passed, 0 failed |
| `kontor-tests-contract --test mcp_parity` | 12 passed, 0 failed |
| `kontor-tests-contract --test runtime_adapter` | 52 passed, 0 failed |
| `kontor-daemon --test loopback_api` | 346 passed, 8 failed, 1 ignored |

The base was run for the same daemon suite: **346 passed, 8 failed, 1 ignored**,
and the sorted failure sets are **identical, zero names added and zero
removed**. The eight are the seven Jira/publication/session-key tests plus
`replaying_a_partial_admission_delivers_its_durable_follow_up`, which fails with
`409 revision_conflict` from a hardcoded task revision as #225 characterized.
They are attributed by measurement at this base, not carried over from an
earlier one.

`cargo fmt`: all five changed files clean. `git diff --check`: clean.
`cargo clippy --all-targets -- -D warnings`: the tree's only error is
`Iterator::last` on a `DoubleEndedIterator` at
`crates/kontor-runtime/src/fake.rs:3618`. That file is not among the five this
change touches, and the identical error was observed at pristine
`origin/master` `86ba6065` with a clean working tree. **No clippy finding is
attributable to this change**; the inherited one is left for ASMA-8203.

## Bounds of this remediation

**The position is held per daemon process, not persisted.** Across a restart the
rotation resumes from the oldest page. This does not reintroduce the finding —
the rotation still advances on every subsequent tick, so nothing is starved
while the daemon runs — but a daemon restarting faster than it completes one
rotation period would keep re-reading the first page. Persisting it durably
needs a new store table and therefore a migration; the realm is at schema 99
after `0099`, migration slots are contended across the active worktrees, and
this seat is barred from deploying or restarting, so a schema change could not
have been exercised against a live realm here. The audit permitted "the smallest
equivalent fair bounded scheme", and this is it. **If durable persistence across
restart is required rather than optional, this specific point is what remains,
and it is a separate change.**

Not done, per instruction: no gate action, no settlement, no Jira, no merge, no
deploy, no restart, no topology action. The realm still runs the combined head
recorded in [DEPLOYMENT-COMBINED.md](DEPLOYMENT-COMBINED.md); this change is
committed and pushed but not deployed.
