# ASMA-8454 desk MCP repair rework candidate

> **Date:** 2026-10-11 22:03 +0200
> **Status:** Frozen rework candidate — same QA/AUD seats and completion committee pending
> **Author:** Codex — ASMA-8454 qualification owner and authorized repair implementer
> **Category:** report
> **Scope:** ASMA-8449 TASK-005; AUD B1 and advisor healthy-native lifecycle; Paseo-direct
> **Summary:** Retains verified directory descriptors through publication and recovery, closes the deterministic ancestry-substitution race, and limits healthy-native replay to pending recovery plus read-only validation. No released acceptance or merge is claimed.

## When to Load

Load when rechecking PR #300 after the rejection of carrier `255d8c936759d1969427842d3919994d7b9dafee`, or preparing the still-held released rehearsal. The original TASK-005 report, operator runbook and discovery realms remain untouched.

## Authority and scope

The coordinator reported QA PASS at 255d8c93, AUD FAIL on blocking B1, and an open advisor lifecycle item, then authorized rework by the same owner on the same branch. These are coordinator-reported gate dispositions, not independent verdicts produced by this seat. The original verdict and previous evidence remain immutable historical inputs. This rework does not reset a gate counter or claim the failed carrier passed. The coordinator routes the same QA/AUD seats at the newer exact head, then the completion committee; no merge is authorized here.

The retained branch is `fix/ASMA-8454-desk-seat-mcp-composition`; PR #300 stays draft. Source checkpoint: baa3f93c89d3d36ea6cb0bb81e681945fec16254 (tree 351481a5790f60b1f85a1a287e3119e9923a8481). The final carrier head/tree and exact carrier gates are supplied separately to avoid embedding a document's own commit hash. The new [source manifest](../../docs/evidence/ASMA-8454/mcp-rework/source-manifest.json), [gates](../../docs/evidence/ASMA-8454/mcp-rework/gates.json), [mutations](../../docs/evidence/ASMA-8454/mcp-rework/mutations/results.json) and [manifest](../../docs/evidence/ASMA-8454/mcp-rework/manifest.json) pin the new evidence. The initial green gates ran at runtime checkout 255d8c93 with explicit dirty-input hashes; all seven match this source checkpoint byte-exact. The final carrier's restored exact-HEAD checks are supplied separately in the coordinator handoff. The previous carrier and its 20/20 mutation results remain historical; the newer results supersede them for these source bytes.

No schema, dependency, shared JSON merge, adapter placement guard, live seat, credential authority, recovery owner, epic pin, watchdog, fleet, production, shared checkout, plan or Jira change is included. The code delta is confined to the composer and hosted launch boundary and their owned tests.

| Qualified runtime/test file | SHA-256 |
| --- | --- |
| `Cargo.lock` | `0eeee22c33074018b56239506d48d03acb329b115af508dc987044c350249a02` |
| `crates/kontor-runtime-paseo/Cargo.toml` | `0dc6bbe90312d2fa95804daea84d060521bcdaaa90d502a06bf66b2d05bf941d` |
| `crates/kontor-runtime-paseo/src/adapter.rs` | `ce59e06f384bd370e5b550cc6739eae74af4445c2e5af3c10e6d9570dad3c570` |
| `crates/kontor-runtime-paseo/src/seat_mcp.rs` | `ca3c41e22ddc4bf3f9297506d11ecc49eb665314f66cfb81e7c069e642b65a97` |
| `crates/kontor-runtime-paseo/src/seat_mcp_directory_tests.rs` | `084f367b03a6dad78c643d8d368c896b125e2808bdaa2d77aa9db6e7ed48dcdc` |
| `crates/kontor-runtime-paseo/tests/contract.rs` | `bc213f1745ae6edd8500d4f943a89a5d176fea76ab79bdf96b6ad664fbc018bd` |
| `crates/kontor-runtime-paseo/tests/contract/hosted_directory.rs` | `238f40e06460462c733182f63c22787225bdb8cbb348b80e8a11a362d4b1a095` |

| New seam or check | File:line |
| --- | --- |
| Retained lineage and identity verification | `crates/kontor-runtime-paseo/src/seat_mcp.rs:439`, `:515` |
| Same-fd source/destination replacement | `crates/kontor-runtime-paseo/src/seat_mcp.rs:651` |
| Healthy pending recovery plus validation | `crates/kontor-runtime-paseo/src/seat_mcp.rs:986` |
| Native health readback and composition choice | `crates/kontor-runtime-paseo/src/adapter.rs:6167`, `:6176`, `:6185` |
| Deterministic B1 interval regression, two publication points | `crates/kontor-runtime-paseo/src/seat_mcp_directory_tests.rs:467` |
| Healthy session/file replay and pending recovery tests | `crates/kontor-runtime-paseo/tests/contract/hosted_directory.rs:534`, `:601` |

## B1: retained descriptors and deterministic regression

Before this correction, `replace_directory_file` opened source and destination parent directories separately by pathname. `NOFOLLOW` protected the final component only. Substituting cwd ancestry between the opens could redirect the destination into a foreign directory. The prior staging-substitution test did not exercise this interval.

The replacement `DirectoryAnchor` descends the canonical absolute path one component at a time with no-follow directory opens and retains the lineage descriptors. Each linked component is checked by device/inode against its retained descriptor. `DirectoryIo` retains cwd, state root and their descended configuration/journal parents for the whole operation. Read/stage/create/replace/remove/fsync operations use those retained descriptors. The atomic rename uses the **same retained parent descriptor** for both source and destination; no separate target pathname open exists. Identity changes refuse before or after the operation and an incomplete rollback retains the journal and blocks create/reload. Rollback and cleanup use the same descriptors, rather than re-resolving the old pathname.

`ancestry_substitution_between_parent_validation_and_replace_is_refused` injects a deterministic ancestry substitution at `ReplaceValidated`, the old source-open/target-open interval. It runs at both settings and MCP replacement points. The foreign directory's exact bytes stay unchanged; the typed refusal reports incomplete cleanup and identity mismatch, the journal remains, and restoring the fixture ancestry allows exact-original recovery and a supported retry. The B1 seed deliberately restores the former separate pathname opens around this seam; the regression detects the foreign-file modification. This is a genuine assertion failure, not a compile failure or timing test.

The existing invalid-second-JSON, 10 absent/populated stage/publish/readback rollback cases, rollback failure, real child exit between publications, completed-pair cleanup, symlink/journal containment and shared-cwd serialization rows run again. This qualifies the Unix implementation on macOS; Windows remains unqualified.

## Advisor lifecycle: healthy replay is read-only after recovery

The launch boundary fetches and verifies the exact recovered agent before choosing composition. Fresh creation and positively identified pre-thread reload (no provider session plus Error/Closed) run full composition. A recovered owned-directory native needing no reload calls `validate_recovered_hosted_directory`: take the same canonical-cwd lock, reconcile a pending journal, then read and validate the required JSON objects without rendering or publishing. Other recovered worktree behavior retains its existing composition policy.

For Claude, the required settings flag must be true and the `kontor` MCP entry must equal the supported command/state-root/leadership entry. Unrelated JSON and formatting differences are accepted and kept byte-exact. Missing, malformed or differing required configuration refuses healthy replay; no file repair, native create or reload occurs. Pending-publication recovery may restore originals or finish cleanup first, as the verdict requires. A partial journal restored to absent originals then refuses missing configuration without publishing a fresh pair. Disabled/foreign provider behavior remains unchanged.

The new healthy tests retain native id and provider session, assert zero extra create/reload and unchanged bytes/mtime/inode for identical files, preserve valid unrelated changes, refuse missing/differing required content without writing, and prove partial-journal recovery without republishing. The pre-thread lost-ack test still proves same-native recovery and one reload. Its recorder now keeps reporting failed health through the first composition refusal; only the successful reload readback reports initialized health. The first gate failure from the stale one-use queued health readback is preserved in `first-gates/`; no gate pass is claimed for that run.

## Credential wording correction

**No new seat credential is written.** Unrelated pre-existing JSON bytes, including secrets already present there, are preserved and may be copied into the private daemon-owned publication journal. The newly supplied scoped seat credential remains in the creation frame's secret channel. The earlier repair report's overbroad renderer/journal wording and matrix line are corrected in this carrier; the original TASK-005 report and operator runbook are unchanged. Synthetic no-new-credential assertions do not establish that arbitrary original JSON contains no secrets.

## Verification

| Gate | Observed result | Log SHA-256 |
| --- | --- | --- |
| paseo | test result: ok. 146 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.56s; test result: ok. 316 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6.31s | `788959b63f013c7d6cfa0151bcadf1084530e6cc41381b5d2d498d96ee6f14e4` |
| fmt | exit 0 | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| clippy | exit 0 | `f8448fb11a0b8311d804a6d5b45b9e63a8f4db2ed216f836d078d72426653e0f` |
| durable_desks | test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 612 filtered out; finished in 2.89s | `5cbf350ef872e5628049845a10952bb046b639904199806f1d263f9af2f3b5a9` |
| durable_seats | test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 608 filtered out; finished in 3.55s | `719e8700ebb129b34cd050394e62843edaff42c93dad1ef321721922aa91db4f` |
| desk_adams | test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 596 filtered out; finished in 11.04s | `f40ce2ca27dc6c9b77ee798ae21e34ef109ea6c8b1821eda59cb11745518fb8d` |
| pr_reviews | test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 606 filtered out; finished in 7.23s | `f439f554acde30ae5bfa42f385300d8ab256cfc33f4368b8ba190ded74ee8f56` |
| cli_parity | test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.95s | `6b35bdd8b62185886c3dbd1cc61753bcd7d9414cdbad12f8a74561eb570ef5fd` |

Exact commands and pinned environment are in the receipts and `reproduce-gates.py`. All commands use the existing owned Cargo target cache, offline locked inputs, owned TMPDIR/Git ceiling, and owned Swagger archive; no fresh full-workspace archive gate is claimed. There are 509 passing tests: 146 runtime unit, 316 adapter contract, 46 four-structure daemon, and 1 CLI parity, plus fmt/clippy. The prior source had 506; the increase is one composer regression and two healthy adapter tests.

**23/23 non-equivalent mutants KILLED; zero survivors or invalid seeds.** Twelve distinct command targets have green baselines. The catalogue retains the original twelve plan/qualification seeds, the previous eight MCP seeds adapted to the new seams, and three new seeds: B1 pathname re-open, healthy replay publication, and skipped healthy recovery/validation.

| Seed | Source file:line | Result |
| --- | --- | --- |
| MUT-001 | `crates/kontor-daemon/src/applications.rs:10755` | KILLED |
| MUT-002-busy | `crates/kontor-core/src/durable_seat.rs:492` | KILLED |
| MUT-002-unchecked | `crates/kontor-core/src/durable_seat.rs:492` | KILLED |
| MUT-002-replay | `crates/kontor-daemon/src/applications/durable_seats.rs:246` | KILLED |
| MUT-003-host | `crates/kontor-daemon/src/applications.rs:10736` | KILLED |
| MUT-003-authority | `crates/kontor-daemon/src/applications.rs:18775` | KILLED |
| MUT-003-ecp | `crates/kontor-daemon/src/applications/desk_adams.rs:78` | KILLED |
| MUT-004-repository | `crates/kontor-store/src/repository/pr_reviews.rs:119` | KILLED |
| MUT-004-close | `crates/kontor-daemon/src/applications/pr_reviews.rs:1040` | KILLED |
| MUT-005 | `crates/kontor-store/src/repository.rs:23903` | KILLED |
| MUT-Q-CLI | `crates/kontor-cli/src/output.rs:81` | KILLED |
| MUT-Q-ATOMIC | `crates/kontor-store/src/repository/desks.rs:48` | KILLED |
| MCP-eligible | `crates/kontor-runtime-paseo/src/adapter.rs:6105` | KILLED |
| MCP-recovered | `crates/kontor-runtime-paseo/src/adapter.rs:6186` | KILLED |
| MCP-ignore-refusal | `crates/kontor-runtime-paseo/src/adapter.rs:6209` | KILLED |
| MCP-write | `crates/kontor-runtime-paseo/src/seat_mcp.rs:1037` | KILLED |
| MCP-journal | `crates/kontor-runtime-paseo/src/seat_mcp.rs:1051` | KILLED |
| MCP-rollback | `crates/kontor-runtime-paseo/src/seat_mcp.rs:1176` | KILLED |
| MCP-cwd-lock | `crates/kontor-runtime-paseo/src/seat_mcp.rs:1045` | KILLED |
| MCP-json-guard | `crates/kontor-runtime-paseo/src/seat_mcp.rs:772` | KILLED |
| MCP-B1-path-reopen | `crates/kontor-runtime-paseo/src/seat_mcp.rs:662` | KILLED |
| MCP-healthy-publish | `crates/kontor-runtime-paseo/src/adapter.rs:6201` | KILLED |
| MCP-healthy-ignore-validation | `crates/kontor-runtime-paseo/src/seat_mcp.rs:986` | KILLED |

Each distinct mutation command has a green baseline. Seeds run individually; every source file and Git status is restored byte-exact. Runtime HEAD labels identify checkout state, while tested input hashes identify dirty source bytes. New B1 and healthy behavior seeds join the original plan/repair catalogue. No seeded source is committed.

## Preserved limits and next owner

Read-only byte inventories prove the held fixture/discovery packages remain unchanged; no SQLite connection was opened for those checks. The original frozen report/runbook hashes still match. Cleanup receipts remove only this rework's own disposable TMPDIR fixtures, with Codex as owner: 25 top-level entries, 159 recursive entries and 76,309,133 regular-file bytes; the path is confirmed absent afterward. Final-carrier gate fixtures receive a separate local cleanup receipt. Original discovery scopes and build caches are excluded.

All previous limits remain: same-vendor LSA→TPM and `keep_native` binding stay refused pending Igor's seat-credential decision; live lost-reply timing is unproven; title collision remains refused; PR close is non-atomic with convergent retry and misleading generic unavailable advice after a partial retirement; the no-proved-container branch remains disclosed. The message-proof TEXT-ordering, shutdown/WAL ownership cycle and epic-first partial-row ordering limitations remain separately scoped follow-ups. Q-002 remains the byte-pinned uncommitted Igor dated decision and its blocked block remains unchecked/unbound.

The four JUDGE wording corrections and the enabled daemon reconcile coverage observation carry forward. This phase proves source adapter behavior with real filesystem and recorded transport, not a released provider process. The full enabled Claude released re-proof, companion MCP identity, four structures, exact successor pair, old-pin/native comparisons, approved real-stop/export-drain/restore procedure, replay, desk-less rollback, partial PR close/retry and new-fixture cleanup remain pending. Reports stays unresolved, and the separately approved runbook docs PR remains unopened until that release record is complete.
