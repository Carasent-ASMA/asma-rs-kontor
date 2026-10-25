# Kontor Local Verification Instead of GitHub Actions CI

> **Date:** 2026-09-01 13:25
> **Status:** 🟢 Approved
> **Category:** architecture
> **Scope:** `asma-rs-kontor` repository verification and merge policy
> **Summary:** Kontor does not run continuous integration in GitHub Actions. Impact-selected verification runs locally against the exact candidate commit and its results are recorded before merge. Igor amended DEC-002 on 2026-10-25; automatic full suites are not the merge gate.

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

  > **Clarification (2026-10-17, checked at `0971674d`):** since PR #198
  > (`840da1b2`, 2026-09-06), hardened in PR #280 (`8acdbd17`), the repository
  > carries one GitHub Actions workflow,
  > [`publication-branch-title.yml`](../../.github/workflows/publication-branch-title.yml).
  > It runs on `pull_request_target` and checks only the pull request's branch
  > name, title key and base against the publication identity rules.
  > It builds, tests, lints and audits nothing and replaces
  > none of the local gates below. This document records no separate decision
  > for it.
- **DEC-002 (amended by Igor, 2026-10-25):** Run tests for changed or added
  behavior and concretely affected callers/contracts only. Do not automatically
  run a full repository, workspace, crate, package or plugin test suite. The
  implementing/integrating seat records the exact head/tree/base, changed-path
  impact analysis, selected tests/commands and applicability dispositions before
  merge. A filename/package match alone is not an impact analysis.

  The dated operator decision was relayed by ROOT and recorded as
  `OPERATOR-DECISION-AND-DISPATCH.json` in the owning ASMA-8539 delivery evidence
  on 2026-10-25. It supersedes the blanket full-suite requirement, not independent
  review, affected security checks, behavioral acceptance or old failure records.
- **DEC-003:** Local verification is required merge evidence. The absence of a
  GitHub status check does not waive, weaken or imply completion of any gate.
- **DEC-004:** GitHub remains the source review and pull-request merge surface;
  this decision changes only where verification executes.
- **DEC-005:** This policy is reversible, but not implicitly. Re-enabling any
  GitHub Actions verification requires a new explicit operator decision, a
  documented rationale, and coordinated updates to this document and the
  contributor-facing instructions.

## Required Local Gate Set

Select and record these obligations at the exact candidate revision:

- Tests for every changed/added behavior and directly affected callers/contracts,
  including cross-package consumers. Select relevant cases from mixed-purpose
  files; a wholly focused test file can run as a file. Shared plugin contracts
  can affect the prototype even when no prototype file changed.
- Relevant build, typecheck, format, dependency/lock and security checks for the
  changed and actually shipped artifact. Check bundled runtime dependencies even
  when a manifest labels them `devDependencies`. Use locked/frozen inputs; never
  regenerate locks as a verification side effect.
- Independent QA and AUD at that head; affected UI/browser evidence and actual
  installed artifact/readback when the release contract requires them.
- Complete qualification of modified Rust executor/contract behavior (including
  ASMA-8641), its affected adapter/daemon callers and integration cases. This
  does not imply testing the entire crate or workspace. A fake adapter cannot
  replace evidence for the actual modified adapter.

Use the existing committed-export verifier with an explicit reviewed impact
plan, as documented in [CONTRIBUTING](../../CONTRIBUTING.md#tests-and-evidence):

```sh
python3 scripts/verify-tree.py --mode archive --impact /tmp/impact.json --receipt /tmp/verification.json
```

The plan names tests, commands, callers/contracts and reasons. Package/dependent
hints from `test-lanes.py --dry-run` are advisory only; JavaScript/prototype
imports and runtime contracts need explicit analysis. No command runs without
an impact plan. Legacy lane names do not authorize broader suites.

The receipt preserves exact candidate/export identity, selected commands/logs,
failures and unrun checks. An exit zero proves only the selected local checks;
external QA/AUD/browser/installed obligations remain separate. Staged/inplace
results are development evidence, not committed-export merge evidence.

Every inapplicable check needs its exact reason and disposition. Distinguish
unchanged/unshipped code with no affected contract from a relevant check that
failed or cannot run; the latter is unresolved, never `not_applicable` or PASS.
Retain failed historical wrappers/receipts as FAIL, along with their actual
scope and any separately owned remediation. Never overwrite or relabel them,
retry until green, add a security waiver, or infer machine contention from a
single isolated pass. Reuse valid exact-head evidence; rerun only missing or
invalidated affected checks. No blanket waiver is introduced.

## Rationale

- **RAT-001:** Kontor delivery executes and records impact-selected checks
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
  branch; the amended delivery workflow records focused local evidence.

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
