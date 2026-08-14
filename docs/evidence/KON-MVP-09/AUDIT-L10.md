# Audit — KON-MVP-09 / ASMA-7753 — KON-20 survivor L10

Date: 2026-08-14  
Seat: Audit  
Audited commit: `f4f9055cabfa8c6310b27692dd8fc122afc07040`  
Parent/archive baseline: `e4242122ee00875231210d061d2fcc07a9dbf90c`

## Overall verdict

**AUDITED_TRUE**

The committed tree is a test-only corrective for the L10 scheduler refusal-
priority finding. No production code or contract changed. The new test pins an
unarmed candidate to `AuthorizationMissing` even when module contention also
applies.

## Checklist

| Area | Verdict | Evidence |
|---|---|---|
| Killer test shape | **PASS** | `crates/kontor-scheduler/tests/ready_batch.rs:636` constructs an unrestricted, unarmed candidate with a shared module. Shape one adds an unrelated in-flight `ModuleClaim`; shape two adds a higher-priority peer that claims the module earlier in the same pass. Both assert `AuthorizationMissing`. |
| `explain()` order | **PASS** | The held-module fixture asserts the complete refusal sequence: `(Authorization, AuthorizationMissing)` followed by `(Contention, ModuleInFlight)`. This proves both blockers are present and that authorization wins by declared blocker order, rather than passing on a single-blocker fixture. |
| Production scope | **CLOSED / PASS** | `git diff e424212..f4f9055` contains exactly one file: `crates/kontor-scheduler/tests/ready_batch.rs`, `80` additions and `1` deletion. Production sources, API code, OpenAPI, and manifests are unchanged. |
| Mutation evidence | **PASS — QA-attested** | The supplied QA result reports both seeded defects (authorization checked only as fallback; authorization removed from the relevant priority path) producing exactly `30 passed, 1 failed`, with only the new killer failing. The committed test structure independently corroborates the claim: the held-lease case forces both `explain()` blockers, and the same-pass peer case forces contention during the admission walk. No mutation was applied to the committed checkout because this audit is read-only. The referenced `QA-L10.md` was not present in this checkout, so the exact two mutation runs are accepted from the supplied QA record rather than claimed as locally rerun. |
| Scheduler gate | **PASS** | `cargo test --offline --locked -p kontor-scheduler --all-targets`: 37 passed, 0 failed (6 `no_seed_branching` + 31 `ready_batch`). |
| Loopback gate | **PASS** | `cargo test --offline --locked -p kontor-daemon --test loopback_api`: 106 passed, 0 failed. |
| Formatting | **PASS** | `cargo fmt --all -- --check` passed. |
| Clippy | **PASS** | `cargo clippy --workspace --offline --locked --all-targets -- -D warnings` passed with no warnings or errors. |
| Cargo.lock | **CLOSED / PASS** | `Cargo.lock` hash is `0cace4c7d2709181c3f863678f30bc0aed5fee73`, identical to `e424212:Cargo.lock`; byte comparison returned equal after all gates. |
| Tree integrity | **CLOSED / PASS** | `HEAD` is the requested commit. No files are staged and no working-tree modifications exist from the audited commit. Preserved untracked `.codebase-memory/` and `docs/evidence/KON-MVP-18/run-a567fe99462e5652/` remain untouched. This audit record is the requested evidence write. |
| Hidden scope cut | **CLOSED / PASS** | The L10 finding is addressed at the scheduler refusal-priority seam only. The test covers both contention shapes and the complete explanation order; there is no production workaround, altered API behavior, removed assertion, or unrelated scope reduction. |

## Conclusion

The L10 survivor is killed by a committed, focused regression test. The tree is
ready for the next audit/integration step. No code edits, staging, commits, or
pushes were performed.

