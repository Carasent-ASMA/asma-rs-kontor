# ASMA-8234 independent refusal-diagnostics verification

> **Date:** 2026-10-03 09:37 CEST
> **Status:** 🟢 Approved
> **Category:** report
> **Scope:** Candidate `6d9f216fc58dd1602f3044f8758c9dac4c907ba8` on `fix/ASMA-8234-refusal-diagnostics`
> **Summary:** Independent source, test, and mutation verification of the prepared refusal-diagnostics candidate. Error semantics, bounded information exposure, and unchanged refusal control behavior all pass.

---

## When to Load

**Load this document when:**

- reviewing ASMA-8234 refusal diagnostics at candidate `6d9f216f`;
- checking whether repository and workspace refusals retain their machine codes while naming the exact static rule;
- tracing the targeted mutation and byte-identical source restoration.

**Do NOT load for:** deployment, release, Jira, or runtime-state evidence; none was performed here.

---

## Result

**PASS.** All three requested properties are established from the exact candidate source and independent execution. No pre-existing or candidate failure occurred in the executed clean-source tests.

This was a verification-only turn authorized as `8190-review-routing` / `fc430da3-e241-41a8-b059-430dea9cd753` under Igor's 2026-10-03 standing authority. It made no source fix, push, PR, merge, Jira, deployment, or runtime change.

## Candidate integrity

| Field | Exact value |
| --- | --- |
| Branch | `fix/ASMA-8234-refusal-diagnostics` |
| Candidate | `6d9f216fc58dd1602f3044f8758c9dac4c907ba8` |
| Parent | `f95e206563bca88b6871f48528623441e5e1a231` |
| Candidate tree | `b795c938aae3468e74a2b0145be61adc56af6c75` |
| Historical source | `fdb6967d` |
| Stable patch-id, both commits | `52a839339d23d400e8759021d56d8aa03436a927` |
| Diff | `crates/kontor-api/src/error.rs` and `crates/kontor-daemon/tests/loopback_api.rs`; 70 insertions, 21 deletions |

The stable patch-id was recomputed independently for both commits and matched. The candidate diff names only the error mapper and its loopback regression; it contains no route, admission, authorization, or permission implementation file.

## Source verification

| Property | State | Exact evidence |
| --- | --- | --- |
| Error semantics | **PASS** | `ApiError::from_repository` at `crates/kontor-api/src/error.rs:608–637` still maps `RepositoryError::Conflict` to `ApiErrorCode::RevisionConflict`; only its `rule` and `subject` become specific. `ApiError::from_runtime` at `:706–828` still maps `RuntimeError::WorkspaceMismatch` to `ApiErrorCode::UnsupportedCapability`; only the fixed generic rule is replaced by the variant's exact rule. `ApiErrorCode` spelling remains `revision_conflict` / `unsupported_capability` (`:42–69`), and HTTP mapping remains 409 / 422 (`:164–183`). Default corrective actions therefore remain unchanged as well. |
| Information exposure | **PASS** | `RepositoryError::Conflict.subject` and `.rule` are both `&'static str` (`crates/kontor-core/src/repository.rs:181–198`). `RuntimeError::WorkspaceMismatch.rule` is `&'static str` (`crates/kontor-runtime/src/adapter.rs:47–97`). `ApiError::new` accepts only a `&'static str` rule, `about` accepts only a `&'static str` subject, and the serialized envelope exposes the same static types (`crates/kontor-api/src/error.rs:305–410`). No caller body, stored value, runtime output, URL, token, or dynamically owned string can enter through the two changed constructors. |
| Unchanged refusal behavior | **PASS** | The candidate changes two mapper expressions and assertions only. It does not alter route selection, admission, authorization, permission checks, status codes, or error variants. The loopback route still returns HTTP 422 and code `unsupported_capability`, now with the exact workspace rule (`crates/kontor-daemon/tests/loopback_api.rs:22356–22379`). The broader API and daemon refusal suites below remain green. |

## Independent test receipts

All commands ran from the candidate worktree with `CARGO_TARGET_DIR=/private/tmp/asma-8234-verify-target`.

| Exact command | Result | Log and SHA-256 |
| --- | --- | --- |
| `cargo test -p kontor-api --lib error` | **15 passed, 0 failed, 14 filtered** | `/private/tmp/asma-8234-api-error-20261003.log` — `1240a2e88cd5f08d86d337ab51bf5a1268ffde60bc308f1f41e0d76fb3e427d1` |
| `cargo test -p kontor-daemon --test loopback_api a_workspace_refusal` | **1 passed, 0 failed, 483 filtered** | `/private/tmp/asma-8234-daemon-workspace-refusal-20261003.log` — `5be69112d0db90437a9e455948d6d4b255808b5663b03536665658d307d2d862` |
| `cargo test -p kontor-api --lib` | **29 passed, 0 failed** | `/private/tmp/asma-8234-api-lib-20261003.log` — `8818b305c95c7b25f80cc35d9d1584f714ab8883e15fc53eb05af8b24c1b80bd` |
| `cargo test -p kontor-api` | **37 passed, 0 failed**: 29 library + 5 envelope + 3 OpenAPI; doc tests 0 | `/private/tmp/asma-8234-api-package-20261003.log` — `83911416b20c32de0ed5bdc76e8d0c53226782aa794d43c1f818859208baea8f` |
| `cargo test -p kontor-daemon --test loopback_api refusal` | **4 passed, 0 failed, 480 filtered** | `/private/tmp/asma-8234-daemon-refusal-20261003.log` — `f6186660fa753b648492c14266de0db356be994d825d9bccf864b624d1d5de3e` |
| `cargo fmt --all -- --check` | **PASS**, no output | `/private/tmp/asma-8234-fmt-20261003.log` — empty-file SHA-256 `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |

No failure was observed in a clean-source run, so there is no baseline failure to separate from the candidate.

## Targeted mutation

Mutant M1 restored the old generic workspace diagnostic in `ApiError::from_runtime`:

```rust
Self::new(
    realm_id,
    ApiErrorCode::UnsupportedCapability,
    "the runtime will not work in the workspace this realm asked for",
)
```

The machine code was deliberately left unchanged; this isolates whether the tests require the new diagnostic rather than merely the same refusal class.

| Killer | Red result | Receipt |
| --- | --- | --- |
| `cargo test -p kontor-api --lib error::tests::a_workspace_refusal_names_the_rule_that_fired` | **KILLED**, exit 101: generic fixed rule differed from `the workspace still reports terminals or another directory` | `/private/tmp/asma-8234-mutant-unit-20261003.log` — `9650ca002ebeb665a2c309e31aecb2757e3e5b4c33824dfc3e92056719f7e776` |
| `cargo test -p kontor-daemon --test loopback_api a_workspace_refusal` | **KILLED**, exit 101: response retained `unsupported_capability` but generic rule differed from `the requested root is not the canonical task worktree of this plane` | `/private/tmp/asma-8234-mutant-daemon-20261003.log` — `d8bc4b6dfd0614f43c2dcf9010c58f03c7cf5942ae3ed792107a19dbc00b34f8` |

## Restoration proof

| File | SHA-256 before mutation | SHA-256 after restoration |
| --- | --- | --- |
| `crates/kontor-api/src/error.rs` | `201e218c3976359a494ab3a1191d102ede94210484e7ba33c83afe387a3ed4d8` | `201e218c3976359a494ab3a1191d102ede94210484e7ba33c83afe387a3ed4d8` |
| `crates/kontor-daemon/tests/loopback_api.rs` | `073c9d5a99b901e50cb404692de9cbbdfa164c2933be59f6546ad0a5e265feea` | `073c9d5a99b901e50cb404692de9cbbdfa164c2933be59f6546ad0a5e265feea` |

After the explicit reverse patch, `git diff --exit-code -- crates/kontor-api/src/error.rs crates/kontor-daemon/tests/loopback_api.rs` returned 0. Both killers were then re-run green: unit 1/0 and loopback 1/0. Combined receipt `/private/tmp/asma-8234-post-restore-20261003.log`, SHA-256 `71dceb2510f18fe3225d53bb51713b58b20e699d040a4e1319e3ea25b6b1b471`.

The five pre-existing untracked adapter entries (`.agents/`, `.asma/`, `.cursor/`, `AGENTS.md`, `CLAUDE.md`) were neither modified nor staged.

## Limits

- This verification ran the complete `kontor-api` package and a focused daemon refusal subset, not the full workspace or full 484-test daemon loopback suite.
- The code graph for this worktree reported stale file metadata, so it was used only for orientation; exact Git diff and source reads are the basis for the negative scope claim.
- Deployment and live runtime behavior were outside authorization and remain untested by this report.

None of these limits leaves a requested property unknown. The candidate is **PASS** for the prepared refusal-diagnostics scope.
