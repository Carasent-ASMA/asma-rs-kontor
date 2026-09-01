# Kontor Local Verification Instead of GitHub Actions CI

> **Date:** 2026-09-01 13:25
> **Status:** 🟢 Approved
> **Category:** architecture
> **Scope:** `asma-rs-kontor` repository verification and merge policy
> **Summary:** Kontor does not run continuous integration in GitHub Actions. The complete verification set runs locally against the exact candidate commit and its results are recorded before merge; changing this policy later requires an explicit decision.

---

## When to Load

**Load this document when:**

- configuring or reviewing automation for the `asma-rs-kontor` repository;
- deciding what verification evidence a Kontor pull request requires;
- considering whether to add or re-enable a GitHub Actions workflow.

**Do NOT load for:** CI/CD policy in another ASMA repository, or Kontor runtime
deployment and release procedures.

---

## Decision

- **DEC-001:** The `asma-rs-kontor` repository does not run CI in GitHub
  Actions. It has no workflow triggered by pushes, pull requests, schedules or
  manual dispatch for build, test, lint, audit or verification gates.
- **DEC-002:** The implementing or integrating seat runs the complete applicable
  gate set locally against the exact candidate commit before merge. The command
  results are recorded in the pull request or owning durable delivery evidence.
- **DEC-003:** Local verification is required merge evidence. The absence of a
  GitHub status check does not waive, weaken or imply completion of any gate.
- **DEC-004:** GitHub remains the source review and pull-request merge surface;
  this decision changes only where verification executes.
- **DEC-005:** This policy is reversible, but not implicitly. Re-enabling any
  GitHub Actions verification requires a new explicit operator decision, a
  documented rationale, and coordinated updates to this document and the
  contributor-facing instructions.

## Required Local Gate Set

Run the gates relevant to the change during development, then run this complete
set from the candidate revision before merge:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo audit
cargo deny check
pnpm install --frozen-lockfile
pnpm --filter kontor-console verify:api
pnpm -r typecheck
pnpm -r test
pnpm audit --prod
python3 scripts/verify-tree.py --mode archive
```

A gate that is genuinely inapplicable or cannot run must be recorded with the
exact reason and disposition; it must not disappear silently from the receipt.
Additional focused, mutation, runtime or release checks remain mandatory when
the affected contract requires them.

## Rationale

- **RAT-001:** Kontor delivery already executes and records the complete gate set
  locally, so push and pull-request workflows duplicate the same verification.
- **RAT-002:** Running on both GitHub `push` and `pull_request` events duplicated
  each branch's hosted work without changing the acceptance boundary.
- **RAT-003:** Keeping the decision explicit prevents a missing status check from
  being mistaken for either accidental misconfiguration or permission to skip
  testing.

## Consequences

### Positive

- **POS-001:** Kontor no longer consumes GitHub Actions time for duplicate gates.
- **POS-002:** The repository has one explicit verification path and one
  evidence obligation.

### Negative

- **NEG-001:** GitHub no longer independently reproduces the candidate in a
  clean hosted Linux environment.
- **NEG-002:** GitHub cannot enforce the local results as required status checks.
- **NEG-003:** Local toolchain or machine drift can affect reproducibility.

### Mitigations

- **MIT-001 (NEG-001, NEG-002):** Persist exact commands, outcomes and the tested
  candidate revision in the pull request or durable delivery evidence.
- **MIT-002 (NEG-003):** Honor the repository's pinned Rust, Node, pnpm and
  dependency locks, use frozen installs, and verify the committed archive.
- **MIT-003 (NEG-001):** Preserve independent review and contract-specific
  mutation/failure evidence; local-only execution does not reduce the acceptance
  criteria.

## Alternatives Considered

### Keep GitHub Actions on push and pull requests

- **ALT-001:** Rejected because it executes duplicate hosted runs for the same
  branch while the delivery workflow already requires the full local suite.

### Keep one GitHub Actions trigger

- **ALT-002:** Rejected for the current policy because even one hosted trigger
  would contradict the explicit choice to execute Kontor verification locally.

### Keep a manual GitHub Actions workflow

- **ALT-003:** Rejected because a dormant manual workflow would make the policy
  ambiguous. A future hosted verification path must be reintroduced by an
  explicit decision rather than left latent.

## References

- [`README.md`](../../README.md) — contributor entry point and required local gates.
- [`CONTRIBUTING.md`](../../CONTRIBUTING.md) — merge evidence and contribution procedure.
