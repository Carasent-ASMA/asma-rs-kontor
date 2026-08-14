# QA-L10 — KON-MVP-09 / ASMA-7753

**Verdict: READY-FOR-AUDIT**

Verified commit: `f4f9055cabfa8c6310b27692dd8fc122afc07040`  
Archive baseline: `e424212`  
Seat: independent QA  
Date: 2026-08-14

## Findings

### Killer test — PASS

`crates/kontor-scheduler/tests/ready_batch.rs:636`
`an_unarmed_candidate_contending_for_a_held_module_is_refused_for_the_arming`
uses one candidate with both `authorization = None` and an unrestricted
calendar, plus a claimed module.

It verifies both contention shapes:

1. An unrelated in-flight module lease.
2. A higher-priority peer that claims the module earlier in the same pass.

Before checking the reported plan reason, the test calls `explain()` and pins
the complete blocker sequence to:

```text
(Authorization, AuthorizationMissing)
(Contention, ModuleInFlight)
```

The plan then must report `AuthorizationMissing` for the candidate in both
shapes. This prevents a vacuous oracle.

### Production-code scope — PASS

`git diff --name-status e424212..HEAD` reports exactly one changed file:

```text
M crates/kontor-scheduler/tests/ready_batch.rs
```

The production refusal path remains `refuse()` in `ready.rs`; it returns the
first blocker in `BLOCKER_ORDER`, where `Authorization` precedes `Contention`.
No production code changed in the killer commit.

### Mutation verification — PASS

Mutation runs were performed in two isolated temporary worktrees, leaving the
committed checkout untouched. Each ran:

```text
cargo test --offline -p kontor-scheduler --test ready_batch --locked
```

Results:

| Mutant | Result |
|---|---|
| Suppress the missing-authorization refusal inside `authorization()` when a module is present | Exit 101; **30 passed, 1 failed**. Only the new killer test failed, at its `explain()` assertion. |
| Defer `Authorization` in `refuse()` and use it only as a fallback after later blockers | Exit 101; **30 passed, 1 failed**. Only the new killer test failed, reporting `ModuleInFlight` instead of `AuthorizationMissing`. |

Temporary mutation worktrees were removed. The target checkout has no source
mutation, staging, commit, or push.

## Gates

| Command | Result |
|---|---|
| `cargo test --offline -p kontor-scheduler --locked` | Exit 0; **37 passed** total (6 `no_seed_branching`, 31 `ready_batch`, doctests clean) |
| `cargo test --offline -p kontor-daemon --test loopback_api --locked` | Exit 0; **106 passed** |
| `cargo fmt --all -- --check` | Exit 0 |
| `cargo clippy --offline -p kontor-scheduler --all-targets --locked` | Exit 0; no warnings/errors |
| `git diff --check` | Exit 0 |

## Tree and lockfile

The target submodule is at `f4f9055cabfa8c6310b27692dd8fc122afc07040` in a
detached worktree. The only remaining status entries are the preserved
untracked `.codebase-memory/` and
`docs/evidence/KON-MVP-18/run-a567fe99462e5652/` paths.

`Cargo.lock` has no diff. Its SHA-256 is byte-identical to the archive:

```text
e424212: fd022e16848992060cac6657706b48e6787575f9f768d886e32a7a4255a59453
worktree: fd022e16848992060cac6657706b48e6787575f9f768d886e32a7a4255a59453
```

## Residual

No residual defect found for L10. The mutation-survivor gap is covered by the
committed test; this change is test-only and is ready for audit.
