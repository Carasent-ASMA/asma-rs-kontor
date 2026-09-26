# ASMA-8196 bound-seat recovery verification

> **Date:** 2026-09-26 05:06 Europe/Oslo
> **Status:** Attachment correction verified; exact-candidate requalification and deployment pending
> **Category:** report
> **Scope:** Core Team rematerialization, exact native occupancy reuse and interrupted launch-intent installation
> **Summary:** A crash after occupancy binding left a Prepared launch intent. Retry previously returned success without installing it. Recovery now proves the exact live native and frozen authority, installs that same intent, and preserves the occupancy generation.

## Candidate and authority

Igor authorized Paseo-direct delivery of Kontor/orchestration recovery. Source baseline is master `7ffd650d9091410887a3de6a360296be9d9a88fd`. The existing ASMA-8196 patch `008b1712316a07394a9c71e4bb09125368dc71ea` was imported onto that baseline, retaining subsequent master fixes. Worktree: `.worktrees/asma-8190/asma-rs-kontor`; epic branch: `feat/ASMA-8190-autonomous-epic-delivery`.

## Resulting behavior

- Rematerializing an already-bound seat reads its current occupancy generation and proves its exact native is live. It creates no replacement native.
- A matching Prepared intent is installed against that proven native. Replays preserve native identity, persona, authority and generation.
- Divergent frozen model/autonomy or an installed intent naming another native is refused.
- Missing natives and route changes remain owned by the audited route replacement path.
- Legacy adopted occupancies with no launch intent retain their provenance.

The new crash fixture aborts the second intent installation after both native occupancies have bound. It then retries through the real router and migrated SQLite store, requiring Installed state, exact native identity, unchanged generation and no additional native creation. Separate cases seed conflicting frozen model/autonomy and require refusal without installation.

## Verification

| Check | Observed result |
| --- | --- |
| Regression before correction | Failed: intent remained `Prepared`, expected `Installed` |
| `cargo test -p kontor-daemon --test loopback_api materializing_ -- --nocapture` | 5 passed |
| Full loopback after fixture correction | 476 passed, 0 failed, 1 existing ignored; 57.88 seconds |
| `cargo clippy -p kontor-daemon --all-targets -- -D warnings` | Passed after exact worktree recovery; 40.58 seconds |
| `cargo fmt --all` and `git diff --check` | Passed |
| Independent QA re-review | Existing Cursor QA `a526c5cc-e399-43c9-8a5e-830e5834a27b` confirmed its prior P1 closed in the candidate. Exact TSW `wks_8d363e5f3188d323` and provider session `656a511e-fd25-4acc-8425-39650f33fc11` preserved. |
| Full archive verification of `ef1475c7` | Workspace Clippy passed; workspace tests stopped at one pre-existing schema rollback fixture collision |
| Corrected legacy-schema fixture (`repository_roundtrip v115_`) | 3 passed; production guards and acceptance assertions preserved |
| Full archive verification of `01f8ad93` | Workspace Clippy and the repaired migration suite passed; schema-shape suite found its deliberately pinned expected version still set to 118 (63 passed, 1 failed) |
| Full archive verification of `72731969` | Exit 0; 2,897 Rust passed, nine existing ignored; 305 console passed; fmt/workspace Clippy/audit/deny/frozen install/typecheck/production audit passed |
| Separate `verify:api` on `72731969` | Passed; initial sandbox DNS refusal preserved separately |
| Attachment regressions against `72731969` | Both Prepared-before-install and Installed-before-attachment recovery failed the durable attachment assertion |
| Corrected materialization contracts after mutation restoration | Six passed; exact native/authority/generation, durable attachment and no invented activity |
| Attachment/installation source mutation verification | Three compiled mutants killed and restored; actual logs preserved below |

Independent QA's actual finding:

> The prior P1 is closed in the uncommitted candidate. A bound seat with a `Prepared` intent is no longer reported as materialized until that intent is installed against the exact live native.

The full loopback initially had six fixture failures: its Git artifact helper mistook an empty `.git` placement marker for an initialized repository. The isolated lifecycle test reproduced the same setup failure. The fixture now asks Git whether a repository exists before skipping initialization; lifecycle assertions and acceptance scope remain intact.

Full archive verification then exposed the schema-v115 fixture's deliberate rollback from the current schema. It undid v116–v118 additions but left the two v119 publication-attestation triggers, so migration replay collided when recreating them. The fixture now removes both guards during rollback, allowing the original restart assertion to verify their installation again. Production migrations are unchanged. Full archive verification must pass on the corrected commit before merge.

The next archive run passed that correction and reached the schema-shape suite. Its deliberate version pin still expected 118 despite merged publication-attestation migration 0119. The expected literal is updated to 119 with the migration purpose recorded; it remains an independent pin rather than being replaced with a tautological comparison. Full qualification then passed on `72731969`; the later attachment correction below requires qualification again on its exact committed head.

## Corrective review: durable attachment

The LSA independently reviewed `72731969`, confirmed the P1 correction and full-gate counts, but found a P2: recovery installed an intent and returned without recording the successful native attachment. A crash after installation but before the normal attachment write had the same defect. The [actual finding](evidence/ASMA-8190/lsa-attachment-review-20260926.txt) is preserved without rewriting its scope.

Two real-router crash cases reproduced the missing-attachment assertion on the reviewed source. Recovery now retains the successful exact-native inspection's timestamp and records it as attachment after intent reconciliation. It writes no activity timestamp and retains the independent activity history. The fixture checks both Prepared and Installed intents, unchanged exact native/generation, no extra creation, durable attachment, and a post-deadline `Stalled` conclusion rather than `AttachmentFailed`. An idle seat's inspection is not proof of activity.

After correction, all six materialization contracts passed. Three separately seeded source defects—omitted attachment recording, invented activity, and omitted Prepared-intent installation—compiled and failed the intended behavioral assertions. Every source mutation was restored and the six contracts passed again. The [manifest](evidence/ASMA-8190/asma-8190-attachment-mutation-results-20260926.json) links exact source/test hashes and actual failure logs; [red](evidence/ASMA-8190/asma-8190-attachment-red.txt) and [restored green](evidence/ASMA-8190/asma-8190-attachment-green-restored.txt) results are retained. Full qualification and independent corrective review remain pending on the new committed candidate before merge.

Original P1 candidate source SHA-256:

- `crates/kontor-daemon/src/applications.rs`: `abb28a293fe308e2fb25f352889cd4e7d1c38e0e302ac5f57cda9c919c9dd929`
- `crates/kontor-daemon/tests/loopback_api.rs`: `760bc4e4db564de6c6dd9b448908ad425372a8eed0bfb4f81c2cf1d207f024d8`

## Worktree recovery incident

ASMA CLI root checkout with `--safe-carry` stashed two sibling Kontor worktrees that share `refs/stash`, then popped the current top stash into ASMA-8190 instead of its own stash. Recovery restored ASMA-8190's two files from exact stash `403d1993`, removed the mistakenly imported fake-runtime change, and restored ASMA-8202's original three files from `0f09fc3b`. Byte comparisons against each saved tree matched. Both stashes and conflict evidence remain preserved. No other agent's work was committed. This CLI defect requires a separate correction before safe-carry can be trusted with shared-repository worktrees.

## Remaining delivery boundary

The original `72731969` passed the complete source gate set; its attachment P2 is corrected and focused verification/mutations pass. The new exact candidate must complete full qualification and corrective review before source merge. Deployment, live persona/attachment readback, final task settlement, ASMA-8190 epic completion and Kontor trust promotion still require their actual receipts and readbacks.
