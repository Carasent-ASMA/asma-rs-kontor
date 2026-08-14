# KON-MVP-23 / ASMA-7821 — Memory-ledger mutation re-QA

Date: 2026-08-14 (Europe/Oslo)

Verdict: **READY-FOR-AUDIT**

Scope: same replacement QA seat, canonical TSW `wks_0fe677df8a595067`; no new seat, Jira, AgentsRoom, or production-code mutation. Frozen implementation commit:

```text
b1863d3ed1ff8f8ae09b69df433fe8d179c943f2
parent/archive: e4242122ee00875231210d061d2fcc07a9dbf90c
message: test(kontor): kill memory ledger mutation survivors (ASMA-7821)
```

## Mutation verification

Baseline focused command on the restored archive:

```text
cargo test -p kontor-store memory::tests --locked
```

Result: **8 passed, 0 failed**.

Each mutant was seeded one at a time in a disposable copy of the archive, the named test was run with `--locked`, and the original source was restored before the next mutant. Every mutant failed its targeted assertion (exit 101); every restored test passed (exit 0).

| Mutant | Seeded defect | Targeted test | Mutant result | Restored result |
|---|---|---|---|---|
| ML01 | `memory_items` conflict path changed from `ON CONFLICT DO NOTHING` to `ON CONFLICT DO UPDATE SET aggregate_revision=0` | `memory::tests::reproposal_never_resets_the_aggregate_revision` | **KILLED**, exit 101; `(0, 2)` vs expected `(2, 2)` | **PASS**, exit 0 |
| ML03 | `memory_history` current flag changed from `i.current_revision_id=r.id` to the approval-exists flag | `memory::tests::two_approvals_leave_exactly_one_current_revision` | **KILLED**, exit 101; current count `2` vs expected `1` | **PASS**, exit 0 |
| ML07 | frozen revision hash changed from `ContentHash::parse(&hash)` to `ContentHash::of(b"missing")` | `memory::tests::frozen_revision_hash_is_the_approved_stored_hash` | **KILLED**, exit 101; frozen hash differed from stored hash | **PASS**, exit 0 |
| ML09 | proposal path additionally inserted a draft into `memory_fts` | `memory::tests::proposal_never_enters_fts_before_approval` | **KILLED**, exit 101; unapproved row count `1` vs expected `0` | **PASS**, exit 0 |

Mutation result: **4/4 killed; 0 survivors**.

## Commit and source-scope verification

```text
git diff --name-status e424212..b1863d3
M    crates/kontor-store/src/memory.rs

git diff --numstat e424212..b1863d3
160    0    crates/kontor-store/src/memory.rs
```

The complete parent delta is one file and 160 additions. The additions are the four `#[test]` functions and their test-only assertions/fixtures in `memory.rs`; no non-test production logic or other source file changed. `git diff --check e424212..b1863d3` passed.

## Required gates

Commands and results on `b1863d3ed1ff8f8ae09b69df433fe8d179c943f2`:

```text
cargo check -p kontor-store
PASS

cargo test -p kontor-store memory::tests --locked
PASS — 8 passed, 0 failed

cargo test -p kontor-store --locked
PASS — 257 passed, 0 failed

cargo clippy -p kontor-store --all-targets --locked -- -D warnings
PASS

cargo fmt --all -- --check
PASS
```

## Cargo.lock and worktree

After all Cargo commands, `git diff --stat -- Cargo.lock` and `git diff -- Cargo.lock` were empty. As the final explicit lock-path repository action, Cargo.lock was restored from frozen HEAD:

```text
git restore --source=b1863d3ed1ff8f8ae09b69df433fe8d179c943f2 -- Cargo.lock
```

Verification:

```text
HEAD_LOCK=0cace4c7d2709181c3f863678f30bc0aed5fee73
WORKTREE_LOCK=0cace4c7d2709181c3f863678f30bc0aed5fee73
cmp <(git show HEAD:Cargo.lock) Cargo.lock — PASS
git diff --exit-code -- Cargo.lock — PASS; no lock diff
```

Final tracked worktree status is clean. Aside from this requested untracked QA evidence file, the remaining status entries are preserved untracked foreign KON-18 evidence directories:

```text
docs/evidence/KON-MVP-18/run-40870492d74e3b3a/
docs/evidence/KON-MVP-18/run-89a688943e1099bf/
docs/evidence/KON-MVP-18/run-97d55adc7ea6a9ef/
```

Residual blockers: **none**.
