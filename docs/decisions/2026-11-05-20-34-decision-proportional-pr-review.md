# Proportional PR review and bounded corrections

> **Status:** Accepted
> **Date:** 2026-11-05 20:34 UTC
> **Authority:** Configured platform owner
> **Scope:** Public PR-review method and merge evidence; task/epic completion and live activation are unchanged.
> **Supersedes:** none
> **Publication class:** P

## Context

A uniform PR committee and nested fix PRs impose avoidable review cycles.
PR acceptance also needs a clear boundary from task/epic completion.
The [PR-review profile](https://github.com/Carasent-ASMA/asma-rs-kontor/blob/master/workflow/skills/pr-review/SKILL.md) owns the
detailed tier, evidence, correction and merge contracts.

<a id="DEC-01"></a>

## Decision

- Adopt scripted Simple, Medium and Complex PR tiers, with risk forcing Complex
  and specialists. Simple/Medium use one independent reviewer; Complex uses
  two reviewers and a JUDGE with three distinct actual vendors. Preserve
  independence from writers, including fallback and delta verification.
- The reviewing side owns corrections by dispatching SWE in isolation.
  Publish guarded appends to the original PR branch; require independent
  delta verification and persist every finding's identity and disposition.
  Bound each PR to three full reviews, two fix attempts and two delta
  verifications. Exhaustion is a handoff, never approval.
- Permit automatic merge only under the configured owner's explicit grant and
  the same current-head/base/criteria review, CI/test and external-approval
  gate as other merge paths. Missing evidence, holds and actionable unresolved
  findings prevent merge. PR acceptance never establishes task/epic Done.
- Keep P10, task QA/AUD, completion committees and closeout unchanged.
  Ownership comes from project configuration. Policy acceptance and publication
  neither enable the live desk nor remove STOP.

## Consequences

The profile, its prompts, glossary, workflow and publication callers must
agree, and adapters must enforce the same contracts. Superseding this decision
requires coordinated changes to those sources and their affected tests.
Review effort becomes proportional; no tier proves the absence of all bugs.
Publication uncertainty and exhausted correction budgets remain visible holds.

## Alternatives considered

- A committee for every PR: unnecessary routine cost without eliminating defects.
- Nested fix PRs: complicate ownership and add a second acceptance lifecycle.
- Writer approval or unbounded retry: breaks independence or hides stalled work.

## Evidence

- Baseline reviewed: `222920fb`; the linked profile and
  [P10](../PRINCIPLES.md) establish the amendment boundary.
- Acceptance: the configured platform owner's explicit 2026-11-05 instruction
  approves implementation of this policy and keeps live activation held.
- Implementation qualification belongs to the delivery change's exact-revision
  review and test evidence; this record claims no live qualification.
