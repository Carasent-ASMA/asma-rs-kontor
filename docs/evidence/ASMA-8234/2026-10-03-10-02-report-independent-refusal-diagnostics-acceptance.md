# ASMA-8234 independent refusal-diagnostics acceptance

> **Date:** 2026-10-03 10:02 CEST
> **Status:** 🟢 Approved
> **Author:** Codex (independent auditor)
> **Category:** report
> **Scope:** Source candidate `6d9f216fc58dd1602f3044f8758c9dac4c907ba8` on `fix/ASMA-8234-refusal-diagnostics`, with verification evidence `ded67076996f83c4f34632e9f69512a9e5a65b52`
> **Summary:** Independent Spec Audit acceptance of the ASMA-8234 refusal-diagnostics residual. The exact error semantics, static-string exposure boundary, unchanged refusal behavior, patch identity, and verifier evidence pass review.

---

## When to Load

**Load this document when:**

- deciding whether the exact ASMA-8234 refusal-diagnostics candidate is accepted;
- checking the independent Verify plus Spec Audit evidence chain for this residual;
- preserving the historical ASMA-8234 findings without backdating acceptance.

**Do NOT load for:** deployment, release, admission, permission, Jira, integration, or live-runtime claims; none was performed or accepted here.

---

## Verdict

**ACCEPT.** The source candidate at `6d9f216fc58dd1602f3044f8758c9dac4c907ba8` is accepted for the bounded refusal-diagnostics scope. There are no new findings.

This is the independent Spec Audit after the independent Verify PASS recorded at `ded67076996f83c4f34632e9f69512a9e5a65b52`. The owning policy in `docs/RECOMMENDED-TEAMS-AND-SEATS.md` requires both roles: QA / Verify supplies `qaPassed`, and Spec Audit independently reads the diff, intent, risks, and evidence to supply the final `auditPassed` verdict. This document supplies the latter source-evidence decision only; it does not write a gate or state in any external system.

## Independence

- The candidate commit records Claude Opus 5 as co-author. This review was performed by a Codex auditor, a different model/vendor role.
- QA identity pin `51b68f89` produced the prior independent verification. This auditor did not author that report, the candidate, or its tests.
- The finding comes from fresh source and diff reads, recomputed identity checks, bounded fresh test execution, and direct inspection of the verifier's red mutation and restoration receipts. It does not rely on the verifier summary alone.

## Exact pins

| Item | Exact value |
| --- | --- |
| Branch | `fix/ASMA-8234-refusal-diagnostics` |
| Base | `f95e206563bca88b6871f48528623441e5e1a231` |
| Accepted source | `6d9f216fc58dd1602f3044f8758c9dac4c907ba8` |
| Historical source | `fdb6967d378cb6bb8475853973b3446b2c23dbf4` |
| Stable patch-id, both source commits | `52a839339d23d400e8759021d56d8aa03436a927` |
| Verification evidence commit | `ded67076996f83c4f34632e9f69512a9e5a65b52` |
| Verification report | `docs/evidence/ASMA-8234/2026-10-03-09-37-report-independent-refusal-diagnostics-verification.md` |
| Verification report SHA-256 | `06e8100b4e30520120a4ab604072da7ab5065403a972618d577dd17555a966d2` |

`ded67076996f83c4f34632e9f69512a9e5a65b52` has parent `6d9f216fc58dd1602f3044f8758c9dac4c907ba8` and adds only the verification report. The base-to-source diff changes only `crates/kontor-api/src/error.rs` and `crates/kontor-daemon/tests/loopback_api.rs`: 70 insertions and 21 deletions. `git diff --check` is clean.

The stable patch-id was recomputed independently from both full patches and matched exactly. No route, admission, authorization, permission, or integration implementation file is in the candidate diff.

## Contract audit

### Exact error semantics — PASS

`ApiError::from_repository` still maps `RepositoryError::Conflict` to `ApiErrorCode::RevisionConflict`; the delta replaces only the generic rule with the variant's exact `rule` and adds its `subject`. The stable machine spelling remains `revision_conflict`, its HTTP mapping remains 409, and its default action remains code-derived.

`ApiError::from_runtime` still maps `RuntimeError::WorkspaceMismatch` to `ApiErrorCode::UnsupportedCapability`; the delta replaces only the generic sentence with the exact variant rule. The stable machine spelling remains `unsupported_capability`, its HTTP mapping remains 422, and its default action is unchanged.

The added unit assertions pin both machine codes and exact diagnostics. The loopback regression still drives the same topology materialization refusal and proves HTTP 422 plus `unsupported_capability`, while now requiring the exact rule `the requested root is not the canonical task worktree of this plane`.

### Bounded information exposure — PASS

The newly surfaced values cannot carry caller or runtime payload bytes:

- `RepositoryError::Conflict.subject` and `.rule` are both `&'static str`;
- `RuntimeError::WorkspaceMismatch.rule` is `&'static str`;
- `ApiError::new` accepts `rule: &'static str`, `ApiError::about` accepts `subject: &'static str`, and the serialized body retains those static types.

The candidate therefore exposes authored rule/subject identifiers, not a prompt, request body, stored value, token, URL, arbitrary runtime output, or dynamically owned string. Existing dynamic fields and sensitive/error catch-alls are untouched.

### Unchanged refusal behavior — PASS

The production delta is confined to two mapper expressions and explanatory comments. The loopback delta strengthens an assertion from a substring match to exact equality. Variant selection, status selection, corrective action selection, route execution, admission, authorization, permission checks, persistence, and runtime effects are unchanged. No broader integration or deployment claim is inferred.

## Verifier evidence spot-check

The committed verification report was read in full. Its recorded clean results are 15/0 for the API error module, 1/0 for the focused workspace refusal, 29/0 for the API library, 37/0 for the API package, and 4/0 for the daemon refusal subset, plus clean formatting.

The two mutation logs were inspected directly and their SHA-256 values recomputed:

| Mutant check | Directly observed result | SHA-256 |
| --- | --- | --- |
| API exact workspace-rule unit test with the old generic rule restored | **KILLED**: 0 passed, 1 failed; expected `the workspace still reports terminals or another directory`, received the old generic sentence | `9650ca002ebeb665a2c309e31aecb2757e3e5b4c33824dfc3e92056719f7e776` |
| Daemon loopback refusal with the old generic rule restored | **KILLED**: 0 passed, 1 failed; code remained `unsupported_capability`, but the exact rule mismatched | `d8bc4b6dfd0614f43c2dcf9010c58f03c7cf5942ae3ed792107a19dbc00b34f8` |

The post-restore receipt SHA-256 is `71dceb2510f18fe3225d53bb51713b58b20e699d040a4e1319e3ea25b6b1b471`; it shows both killers green again, 1/0 each. Current source hashes independently match the report's before/after restoration pins:

- `crates/kontor-api/src/error.rs`: `201e218c3976359a494ab3a1191d102ede94210484e7ba33c83afe387a3ed4d8`
- `crates/kontor-daemon/tests/loopback_api.rs`: `073c9d5a99b901e50cb404692de9cbbdfa164c2933be59f6546ad0a5e265feea`

This is sufficient mutation evidence for the changed workspace diagnostic: restoring the exact old behavior is detected at both the mapper and HTTP boundary, and restoration is byte-identical.

## Fresh audit checks

All commands ran from `/Users/igor/carasent/asma-modules/.worktrees/asma-8234/asma-rs-kontor` with `CARGO_TARGET_DIR=/private/tmp/asma-8234-audit-target` for the tests.

| Exact check | Result |
| --- | --- |
| `cargo test -p kontor-api --lib error -- --test-threads=1` | **15 passed, 0 failed, 14 filtered**; log SHA-256 `6e12b05ac70f93aaf4456f2f32052aa0f3285669be30b5eaada45661bb69ee8f` |
| `cargo test -p kontor-daemon --test loopback_api -- --test-threads=1 --exact a_workspace_refusal_is_reported_as_a_placement_fact` | **1 passed, 0 failed, 483 filtered**; log SHA-256 `0a93d2ad252340dcb4e74272732e5e25a20940dc5151b9171dfd1a4b41591395` |
| `cargo fmt --all -- --check` | **PASS**, no output; empty-file SHA-256 `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| Stable patch-id for source and historical source | Both `52a839339d23d400e8759021d56d8aa03436a927` |
| Base-to-source diff and `git diff --check` | Exactly two files, 70 insertions / 21 deletions; clean |

The focused executions are intentionally bounded. The verifier's broader 29/0, 37/0, and 4/0 clean runs were consumed as evidence and were not duplicated after their receipts and mutation strength were spot-checked.

## Preserved historical record

This acceptance does not rewrite or reopen earlier ASMA-8234 decisions:

- The earlier failed finding named **`redundant adoption branch`** remains a failed historical finding under its original scope and verdict. This refusal-diagnostics residual is not asserted to remediate it.
- The historical graph recorded no verification verdict and no audit acceptance for this task's refusal-diagnostics residual. That historical absence is preserved; the current verification and this audit are new evidence pinned to the exact current candidate, not backdated records.
- Every prior verdict remains attached to the source and scope it originally judged. The current QA PASS applies to `6d9f216f` through evidence commit `ded67076`; this ACCEPT applies only to the same refusal-diagnostics source.

No dissent is added. The preserved historical finding is not a dissent against this bounded candidate.

## Boundaries

This turn made no source change and no deployment, push, PR, merge, Jira, admission, permission, gate, or integration write. It makes no live-runtime or release claim. The pre-existing untracked adapter entries `.agents/`, `.asma/`, `.cursor/`, `AGENTS.md`, and `CLAUDE.md` were left untouched and excluded from the evidence commit.
