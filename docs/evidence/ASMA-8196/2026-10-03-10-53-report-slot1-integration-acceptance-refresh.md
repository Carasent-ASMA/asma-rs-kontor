# ASMA-8196 slot-1 integration acceptance refresh

> **Date:** 2026-10-03 10:53 CEST
> **Status:** 🟢 Approved — acceptance extended
> **Author:** Codex AUD seat
> **Category:** report
> **Scope:** Narrow integration-delta review of merge `5763499d680ccb24c37e34aac80c600195cc4453`
> **Summary:** Confirms that the additive join onto default preserves the accepted ASMA-8196 source byte-for-byte, carries the two default commits intact, and remains green on formatting and both focused regressions.

---

## When to Load

**Load this document when:**

- deciding whether acceptance at `a59d398a` extends to slot-1 integration candidate `5763499d`;
- checking the exact lane/default content of the additive merge.

**Do NOT load for:** push, PR, merge-to-default, deployment, Jira, admission, permission, or live-runtime claims.

---

## Verdict

`accept`

The existing ASMA-8196 acceptance extends to slot-1 integration candidate `5763499d680ccb24c37e34aac80c600195cc4453`. F-1 remains closed and there are no new findings.

## Lane preservation

`git diff a59d398a11bd915e73845f7de1e36dd39bfe975e..5763499d680ccb24c37e34aac80c600195cc4453 -- crates/kontor-api crates/kontor-daemon` exits 0 with no output. The six accepted source blobs match byte-for-byte:

| Path | Blob at both heads |
| --- | --- |
| `crates/kontor-api/contract/openapi.json` | `067cf292b889ffca293ca2f909bc018725b41506` |
| `crates/kontor-api/src/applications.rs` | `d05678665252adb518a7c3ead4ed4b54538e9fd2` |
| `crates/kontor-api/src/lib.rs` | `322362bfb43a53192f107e22137e5ade87a442b6` |
| `crates/kontor-api/src/openapi.rs` | `508095dfd0ccb12a324e9557fc7ab89e6e58bce9` |
| `crates/kontor-daemon/src/applications.rs` | `b4b97f395bb39579f401ac230128db2dcdb998d0` |
| `crates/kontor-daemon/tests/loopback_api.rs` | `772fab8ac8df5dfe4e57b613b94eaba5b984578c` |

The accepted ancestry is intact. Each adjacent `git merge-base --is-ancestor` check returns 0:

`0612a839` → `c56b80eb` → `940138c5` → `a59d398a` → `8144da86` → `5763499d`.

The merge commit has the exact declared parents, lane `8144da86e3607b34e30ca08b9684e5f56c0f2994` and default `4d365dd5de649c68aaabe8c2b67351ae7dd5dbb0`. No accepted commit was replaced or rewritten.

## Default-side preservation

`8acdbd17e83d9d7ec545221c4b957e0325916997` is the direct parent of `4d365dd5de649c68aaabe8c2b67351ae7dd5dbb0`. The four #280 files have identical blobs at `8acdbd17`, `4d365dd5`, and `5763499d`:

| Path | Blob |
| --- | --- |
| `.github/scripts/publication_branch_title.py` | `19526566edebb859cca5f97e1c5dd30fae40a89c` |
| `.github/scripts/tests/fixtures/publication_branch_title_cases.json` | `3ce62f9db4f12b248890f8c66965245dcab46bf1` |
| `.github/scripts/tests/test_publication_branch_title.py` | `a8b93580feee5c9b1a1837e1263cc9b376d56b5b` |
| `.github/workflows/publication-branch-title.yml` | `4065aafe7f1673365d4ca2eafb404f13080d4f14` |

`Cargo.lock` is blob `6642ded3accde407157fd50dbcedd50e2b9be327` at both `4d365dd5` and `5763499d`; its `yoke-derive` entry is version `0.8.4`. Diffing the merge against default for `.github` and `Cargo.lock` is empty.

Re-running `git merge-tree --write-tree 8144da86 4d365dd5` exits 0 and produces tree `eecafa6f239d6c9a6ffb3fb1a79c16d523a7ee17`, exactly the tree recorded by `5763499d`. This establishes a conflict-free additive join with no merge-only resolution.

## Exact checks

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS, exit 0, no output; log SHA-256 `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `cargo test -p kontor-daemon --test loopback_api -- --test-threads=1 --exact a_claim_superseding_a_launched_occupancy_reports_a_null_roster_persona` | PASS — 1 passed, 0 failed, 484 filtered; log SHA-256 `5cc7ff3f0c21dc669e18552cdfa4a390ad91d53bbf6f7afc058e66834071344a` |
| `cargo test -p kontor-daemon --test loopback_api -- --test-threads=1 --exact a_promotion_creates_one_epic_and_hands_the_work_to_its_lsa` | PASS — 1 passed, 0 failed, 484 filtered; log SHA-256 `f9b3e8bb7d3ead6c07db5c22224d48ae1f90940ba90da0601e084495aa0c0238` |

## Boundaries

This review changed no source or existing evidence file. It performed no push, PR, merge, Jira action, deployment, admission, permission, or external gate action. The pre-existing adapter entries remain untouched and excluded from this evidence checkpoint.
