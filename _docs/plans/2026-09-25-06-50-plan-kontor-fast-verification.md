---
goal: Every affected-package verification of a Kontor change completes in under two minutes on a warm worktree, the complete workspace test set completes in under ten minutes when run once, and no delivery seat compiles the workspace cold per task.
version: 0.1
date_created: 2026-09-25
last_updated: 2026-09-25
owner: Igor Efrem (manual orchestration through Paseo, one delivery team)
status: Planned
tags: [kontor, testing, performance, verification, sqlite, nextest, ASMA-8258]
---

# Kontor Fast Verification: Sub-Two-Minute Per-Task Tests and One Full Gate

> **Date:** 2026-09-25 06:50
> **Status:** 🔵 Planned
> **Author:** Igor Efrem (AI-assisted, Claude Fable 5.1)
> **Category:** plan
> **Scope:** `_tools/asma-rs-kontor` build configuration, test harnesses, verification scripts and contributor policy, plus the operator Mac's cargo and sccache configuration
> **Summary:** Verification of one Kontor change costs 40 to 50 minutes because every seat compiles the workspace cold for the archive gate and because the bundled SQLite is built unoptimised with its allocation mutex on, which makes the heavy store suites 10 to 30 times slower than they need to be. This plan fixes the build configuration first, then the runner, fixtures and binaries, then the verification policy, so an affected-package check takes under two minutes and the full gate runs once per candidate commit.
> **Jira:** [`ASMA-8258`](https://carasent.atlassian.net/browse/ASMA-8258)
> **Orchestration:** Paseo direct, one delivery team, managed manually by Igor. The Kontor control plane is deliberately not used for this work (operator decision, 2026-09-25); no Kontor epic, task or receipt exists for it.

---

## When to Load

**Load this document when:**

- Changing how Kontor tests are built, selected, run or timed (`Cargo.toml` profiles, `.cargo/config.toml`, `.config/nextest.toml`, `scripts/test-lanes.py`, `scripts/verify-tree.py`).
- Editing the store or daemon test harness template (`crates/kontor-store/tests/support/mod.rs`, `crates/kontor-daemon/tests/harness/mod.rs`).
- Deciding what verification evidence a Kontor pull request needs, or revisiting the local-verification policy.
- Picking up any `FV-` work item under `ASMA-8258`.

**Do NOT load for:** Kontor runtime behaviour, Jira or Paseo orchestration features, or test failures unrelated to speed.

---

![Status: Planned](https://img.shields.io/badge/status-Planned-blue)

Kontor delivery seats verify every change locally. The governing
[local verification policy](../architecture/2026-09-01-13-25-architecture-kontor-local-verification-policy.md)
requires the complete gate set per candidate commit, including
`scripts/verify-tree.py --mode archive`, which extracts `git archive HEAD` into a
temporary directory and compiles all 756 packages from nothing. Fifty-nine
Kontor worktrees share one 10-core Mac, so those cold compiles collide. On top
of that, the heaviest suites were measured on 2026-09-25 to run *slower in
parallel than serially*: the bundled SQLite is compiled at `-O0` by the `cc`
crate, and its default memory-statistics mutex serialises every allocation of
every test thread inside the kernel.

| Suite (tests) | Recorded in `test-timings.json` | Measured 2026-09-25, baseline | Same build, two flags (FV-01) |
|---|---|---|---|
| `kontor-store` `schema_v1` (64) | 232.4 s | 198.0 s wall, 184 s user, **1110 s sys** | **7.7 s** |
| `kontor-store` `backup_export` (18) | 155.1 s | | **12.8 s** |
| `kontor-store` `team_definition_migration_completeness` (20) | 88.1 s | | **3.1 s** |
| `kontor-store` `backup_snapshot` (12) | 68.7 s | | **4.1 s** |
| `kontor-daemon` `loopback_api` (459) | 56.0 s | | **23.8 s** |
| `kontor-accounts` `account_security` (22) | 71.4 s | 73.2 s wall, 68 s user, **420 s sys** | not yet rebuilt |

The sum of all recorded test execution is 17.8 minutes over 148 targets, run
one binary at a time. 330 of the 756 packages exist only for
`apps/desktop/src-tauri`, which has no tests. The lane script already
computes affected packages; it is simply not the path seats are told to use.

This plan lands the flags first, because they are two lines and remove the
bulk of the test time, then makes the runner, fixtures and binaries cheap,
then changes where the full gate runs.

## 1. Requirements & Constraints

- **REQ-001**: `python3 scripts/test-lanes.py --lane fast` on a change under `crates/kontor-store/` finishes in under 120 s on a warm worktree, proven by its JSON receipt.
- **REQ-002**: The complete workspace test set (every default member, doctests included) finishes in under 600 s when run once on the primary checkout.
- **REQ-003**: `schema_v1`, `backup_export`, `team_definition_migration_completeness` and `backup_snapshot` each finish under 15 s inside the workspace run.
- **REQ-004**: No seat instruction, CONTRIBUTING step or lane path runs `verify-tree.py --mode archive` per task. The full gate runs once per candidate commit through the path FV-08 selects.
- **REQ-005**: Every test that exists today still exists and still asserts the same invariant. No test is deleted or weakened by this plan.
- **REQ-006**: Production SQLite durability is unchanged: WAL, `synchronous` default, `foreign_keys`, busy timeout default and the single-transaction migration remain exactly as in `crates/kontor-store/src/migrations.rs`.
- **REQ-007**: The archive gate, when it runs, still verifies the exact candidate commit from a tree without `.git`.
- **SEC-001**: `.cargo/config.toml` in the repository may set only `[env]` and `[build]` keys that are safe for every contributor; no credentials, no absolute machine paths. Machine-specific paths (sccache base directories, jobserver FIFO) stay in `~/.cargo/config.toml` or the seat environment.
- **SEC-002**: The GitHub Actions workflow in FV-08 (if selected) uses `permissions: contents: read` only and no repository secrets.
- **CON-001**: Orchestration is Paseo direct. One delivery team executes every phase; Igor manages placement, turns and merges by hand. No Kontor task, receipt, gate record or epic is created for this work.
- **CON-002**: Delivery branch `perf/ASMA-8258-fast-verification` from `origin/master` in a fresh Kontor worktree. PR title and branch carry exactly one Jira key (`publication-branch-title.yml`).
- **CON-003**: Conventional Commit subjects with the type first; the Jira key belongs in the body.
- **CON-004**: `Cargo.lock` is not refreshed by this plan. New dev tooling (nextest) is a binary install, not a dependency.
- **CON-005**: The `unsafe_code = "deny"` workspace lint stays. No test calls `sqlite3_config` or other FFI directly.
- **CON-006**: Never kill processes by command-line pattern; several sessions run identical cargo commands in sibling worktrees.
- **GUD-001**: Measure before and after every phase with `/usr/bin/time -p` on the test binary and record wall, user and sys seconds in §6.
- **GUD-002**: Prefer configuration over code, and code over new dependencies.
- **PAT-001**: One integration-test binary per crate (`tests/main.rs` with modules) is the Cargo pattern for large suites.
- **PAT-002**: Fixtures shared across processes live under `CARGO_TARGET_TMPDIR`, keyed by a content digest, created under a file lock and renamed into place atomically.

## 2. Implementation Steps

**Execution model.** One Paseo delivery seat works the phases in the order of
the Plan-Graph waves in §2.5, one phase per turn, and ends each turn with the
phase's measurement rows filled in §6 and a commit on the delivery branch.
Igor reviews the receipt, then sends the next phase. FV-08 waits for Igor's
written decision inside this document before any task in it starts.

### Implementation Phase 1: build configuration (FV-01, FV-02)

- GOAL-001: The bundled SQLite is optimised with memory statistics off in every dev and test build, and the default gate no longer compiles the desktop application.

| Task | Description | Completed | Date |
|------|-------------|-----------|------|
| TASK-001 | Create `.cargo/config.toml` at the repository root with `[env] LIBSQLITE3_FLAGS = "-DSQLITE_DEFAULT_MEMSTATUS=0"` and a comment citing this plan and the SQLite recommended-options list. `libsqlite3-sys` `build.rs` reads that variable and declares `rerun-if-env-changed`, so the change rebuilds the C library once. | | |
| TASK-002 | Add to the root `Cargo.toml`: `[profile.dev.package.libsqlite3-sys] opt-level = 3`, with a comment giving the measured 198 s → 7.7 s figure. Release builds are already optimised; this affects dev and test only. | | |
| TASK-003 | Measure: `cargo test -p kontor-store --test schema_v1 --no-run` then `/usr/bin/time -p target/debug/deps/schema_v1-*` (newest binary). Record wall/user/sys in §6 TEST-001. Pass condition: wall under 15 s and sys under 10 s. Repeat for `backup_export`, `backup_snapshot`, `team_definition_migration_completeness`, `kontor-accounts` `account_security`. | | |
| TASK-004 | Add `default-members` to `[workspace]` in `Cargo.toml` listing every member except `apps/desktop/src-tauri`. Verify with `cargo metadata --format-version 1 --no-deps` that `workspace_default_members` excludes `kontor-desktop`. | | |
| TASK-005 | In `scripts/verify-tree.py` `run_gates`, replace `cargo clippy --workspace --all-targets -- -D warnings` with `cargo clippy --all-targets -- -D warnings` and `cargo test --workspace --locked` with the default-member equivalent, then add a separate gate `cargo check -p kontor-desktop --locked` so the desktop still compiles on every full run without its 330 packages entering the test build. | | |
| TASK-006 | In `scripts/test-lanes.py` `classify`, add `desktop_check = True` when any changed path starts with `apps/desktop/`; in `lane_commands` append `["cargo", "check", "-p", "kontor-desktop", "--locked"]` for that flag in the fast lane. Update the module docstring. | | |
| TASK-007 | Record the number of packages a warm `cargo test --no-run` compiles before and after TASK-004 (`cargo build --timings` unit count) in §6 TEST-002. | | |

### Implementation Phase 2: shared on-disk template fixture (FV-04)

- GOAL-002: The migrated realm template is built once per machine per migration digest, shared by every test process of both the store and daemon suites, and never faked.

| Task | Description | Completed | Date |
|------|-------------|-----------|------|
| TASK-008 | In `crates/kontor-store/src/migrations.rs` add `pub fn schema_fingerprint() -> String`: SHA-256 (workspace `sha2`) over `SCHEMA_VERSION` followed by every entry of `MIGRATIONS` in order, hex encoded (workspace `hex`). Unit test: the value changes when any migration byte changes (test by hashing a modified copy of the inputs, not by editing files). | | |
| TASK-009 | Add a `test-support` cargo feature to `crates/kontor-store/Cargo.toml` exposing `pub mod test_support` with `pub fn template_database() -> PathBuf`: directory `Path::new(env!("CARGO_TARGET_TMPDIR"))` is not available inside the library, so the function takes the cache root as an argument and the two harnesses pass `env!("CARGO_TARGET_TMPDIR")`. Path is `<root>/kontor-template/<fingerprint>/kontor.db`. If present, return it. Otherwise take an exclusive `fs4` lock on `<root>/kontor-template/<fingerprint>.lock`, re-check, migrate a fresh realm with `SqliteStore::open` into `<fingerprint>.building/kontor.db`, assert the `-wal` file is empty (existing invariant), run `PRAGMA integrity_check` and `PRAGMA user_version == SCHEMA_VERSION` on the file, then `rename` the directory into place. | | |
| TASK-010 | Add `kontor-store = { path = ".", features = ["test-support"] }` under `[dev-dependencies]` of `kontor-store` and `features = ["test-support"]` on the existing `kontor-store` dev-dependency of `kontor-daemon`. Replace the `OnceLock<TempDir>` bodies in `crates/kontor-store/tests/support/mod.rs` `migrated_state_root` and `crates/kontor-daemon/tests/harness/mod.rs` `migrated_state_root` with a copy from `template_database(env!("CARGO_TARGET_TMPDIR"))`. Keep every existing helper signature (`state_root`, `store_from_template`, `open_created_realm`, `created_state_root`, `install_migrated_database`). | | |
| TASK-011 | Tests in `crates/kontor-store/tests/support_template.rs` (folded into `tests/main.rs` by Phase 4): (a) ten threads calling `template_database` on an empty root produce exactly one directory and one file; (b) the returned file has `user_version == SCHEMA_VERSION`, `integrity_check == ok`, exactly one `realm_metadata` row; (c) a corrupted `.building` leftover from a killed process is ignored and rebuilt. | | |
| TASK-012 | Mutation task for this phase (record in §6): MUT-001 skip the integrity check; MUT-002 return the `.building` path before rename; MUT-003 hash only `SCHEMA_VERSION`. Each must be KILLED by TASK-011 or TASK-008 tests. | | |

### Implementation Phase 3: nextest as the lane runner (FV-03)

- GOAL-003: Every lane runs tests process-per-test across all binaries in parallel, with per-test timings and slow-test alarms, and doctests still run.

| Task | Description | Completed | Date |
|------|-------------|-----------|------|
| TASK-013 | Install `cargo-nextest` on the operator Mac (`cargo install cargo-nextest --locked`, or `brew install cargo-nextest`). Record the version in `CONTRIBUTING.md` prerequisites. | | |
| TASK-014 | Add `.config/nextest.toml`: `[profile.default]` with `fail-fast = false`, `slow-timeout = { period = "20s", terminate-after = 3 }`, `status-level = "slow"`; `[profile.default.junit] path = "junit.xml"`; `[profile.lanes]` inheriting default. | | |
| TASK-015 | `scripts/test-lanes.py`: `cargo_test_command` returns `["cargo", "nextest", "run", "--profile", "lanes", "--locked", *(-p per package)]` followed by a second command `["cargo", "test", "--doc", "--locked", *(-p)]` because nextest does not run doctests. If `cargo nextest --version` fails, exit 2 with the install instruction. | | |
| TASK-016 | `scripts/test-lanes.py`: replace the libtest regex parsing in `parse_log` and `record_timings` with a JUnit reader over `target/nextest/lanes/junit.xml` (`xml.etree`): per `<testcase>` name, classname (binary), time, failure text. Aggregate per binary for the receipt's `results` rows and for `scripts/test-timings.json`; add a `tests` list with the ten slowest tests per lane run to the receipt. Keep the doctest run parsed by the old `RESULT_RE` path. | | |
| TASK-017 | Lower `HEAVY_THRESHOLD_SECONDS` to 20.0 and drop `kontor-store` and `kontor-daemon` from `FALLBACK_HEAVY_PACKAGES` once TASK-003 shows them under the threshold; keep `kontor-tests-e2e`. | | |
| TASK-018 | `scripts/verify-tree.py` `run_gates`: `cargo nextest run --locked` (default members) plus `cargo test --doc --locked`, keeping fmt, clippy, audit, deny, desktop check and the pnpm gates. | | |
| TASK-019 | Add `scripts/tests/test_lanes.py` (`unittest`) covering: package classification with the desktop flag, JUnit aggregation with one failing test, heavy-threshold selection from a fixture timings file. Add `python3 -m unittest discover -s scripts/tests` to `run_gates`. | | |
| TASK-020 | Mutation task: MUT-004 make the JUnit reader ignore `<failure>` elements; MUT-005 make the desktop flag never set. Both KILLED by TASK-019. | | |

### Implementation Phase 4: one integration binary per crate (FV-05)

- GOAL-004: `kontor-store` and `kontor-core` each link one integration-test binary instead of 33 and 18, with the same test inventory.

| Task | Description | Completed | Date |
|------|-------------|-----------|------|
| TASK-021 | Record `cargo nextest list -p kontor-store` and `-p kontor-core` test counts before the change (expected 579 and 330 across integration and unit targets; record the exact split). | | |
| TASK-022 | `crates/kontor-store/tests/main.rs`: `mod support;` once, then one `mod <file_stem>;` per existing integration file. Set `autotests = false` in `crates/kontor-store/Cargo.toml` and add `[[test]] name = "store" path = "tests/main.rs"`. Resolve name clashes only by renaming private helpers inside the offending module; never rename a test function. | | |
| TASK-023 | Same for `crates/kontor-core/tests/main.rs` with `[[test]] name = "core"`. | | |
| TASK-024 | Verify `cargo nextest list` counts equal TASK-021 exactly, `cargo clippy --all-targets -- -D warnings` is clean, and `cargo build --timings` shows the two crates' test-target link units reduced to one each. Record link seconds before and after in §6 TEST-005. | | |
| TASK-025 | Update `scripts/test-lanes.py` `record_timings` file-to-package mapping so a `tests/main.rs` binary named `store` or `core` maps to its package (the JUnit classname carries the binary name). | | |

### Implementation Phase 5: injectable timeouts for the two real waits (FV-06)

- GOAL-005: The two tests that genuinely wait 30 s prove the same timeout behaviour in under 2 s, with production defaults unchanged.

| Task | Description | Completed | Date |
|------|-------------|-----------|------|
| TASK-026 | `crates/kontor-store/src/migrations.rs`: keep `BUSY_TIMEOUT` as the default and add `pub struct OpenOptions { pub busy_timeout: Duration }` with `Default` = 30 s, plus `SqliteStore::open_with(path, OpenOptions)`; `SqliteStore::open` delegates with the default. Thread the value through `configure_connection` and the lock-deadline arithmetic (`lock_deadline = now + 2 * busy_timeout`). | | |
| TASK-027 | `crates/kontor-store/tests/schema_v1.rs` `a_busy_writer_waits_then_times_out_without_partial_state`: open the waiting side with `busy_timeout = 1 s`; assert the reported `PRAGMA busy_timeout` equals the configured value, not the constant. Keep `every_connection_reports_wal_foreign_keys_and_a_bounded_busy_timeout` asserting the production default through `SqliteStore::open`. | | |
| TASK-028 | `crates/kontor-jira`: make the connector's request timeout a constructor parameter with the current 30 s default; in `tests/native_connector.rs` the two tests using `Duration::from_secs(31)` use a 1 s timeout and a 2 s wiremock delay. | | |
| TASK-029 | Mutation task: MUT-006 ignore the configured busy timeout and use the constant; MUT-007 ignore the configured request timeout. Both KILLED (the tests fail or trip nextest's slow-timeout termination). | | |

### Implementation Phase 6: machine hygiene for parallel seats (FV-07)

- GOAL-006: Parallel seats on the operator Mac share compiler cache hits, do not oversubscribe the ten cores, and retired worktrees do not keep multi-gigabyte targets.

| Task | Description | Completed | Date |
|------|-------------|-----------|------|
| TASK-030 | `~/.cargo/config.toml` `[env]`: add `SCCACHE_BASEDIRS` listing the worktree roots in use (`/Users/igor/carasent/asma-modules/.worktrees`, `/Users/igor/.local/state/kontor/asma/repair-worktrees`, `/Users/igor/carasent/asma-rs-kontor.worktrees`, `/private/tmp`) and `SCCACHE_DIRECT = "false"` (sccache 0.18.0 has a known wrong-object bug in preprocessor cache mode with basedirs). Restart the sccache server. | | |
| TASK-031 | A/B: build `cargo test -p kontor-store --no-run` in a second fresh worktree after a warm first one; record `sccache --show-stats` hits/misses and wall time before and after TASK-030 in §6 TEST-007. Keep the setting only if cross-worktree Rust hits appear. | | |
| TASK-032 | Add `scripts/jobserver.sh`: create a FIFO under `~/.local/state/kontor/`, preload it with ten tokens, and print the `MAKEFLAGS="--jobserver-auth=fifo:<path>"` line seats export. Verify with two simultaneous `cargo build --no-run` runs in two worktrees that the concurrent `rustc` count stays at or below ten (`ps` count, not `pkill`). Document that cargo honours the inherited jobserver only when `-j`/`build.jobs` is unset. | | |
| TASK-033 | Add `scripts/prune-worktree-targets.py`: list every Kontor worktree (`git worktree list --porcelain`) whose branch is merged or whose checkout is older than 14 days, print `target/` sizes, and with `--apply` run `cargo clean` in each listed one. Dry-run by default. | | |
| TASK-034 | Write `docs/DEVELOPMENT-MACHINE.md`: the flags, the jobserver, sccache base directories, `nice`, `RUST_TEST_THREADS` guidance for seats, and the prune script. Link it from `CONTRIBUTING.md`. | | |

### Implementation Phase 7: verification policy (FV-08, decision-gated)

- GOAL-007: The full gate runs once per candidate commit outside every seat's inner loop, and the policy documents say so.

**Decision gate.** Igor writes one of the two lines below into this section before any task in this phase starts. Until then every task here is `blocked`, whatever the graph says.

- Decision (fill in): `FV-08 option A selected on <date>` or `FV-08 option B selected on <date>`.

| Task | Description | Completed | Date |
|------|-------------|-----------|------|
| TASK-035 | **Option A (recommended): hosted merge-queue gate.** Write `_docs/architecture/2026-<date>-architecture-kontor-hosted-full-gate.md` superseding the 2026-09-01 policy (mark that document ⚫ Superseded and update the router). Add `.github/workflows/verify.yml` on `pull_request` and `merge_group`: `ubuntu-latest`, `permissions: contents: read`, toolchain from `rust-toolchain.toml`, `Swatinem/rust-cache`, `taiki-e/install-action@nextest`, then the exact `verify-tree.py --mode inplace` gate list. Igor enables the merge queue and marks `verify` a required check. | | |
| TASK-036 | **Option B: local single-flight full gate.** Add `scripts/full-gate-queue.sh`: `flock` on one machine-wide lock, `CARGO_TARGET_DIR=$HOME/.local/state/kontor/full-gate-target` (warm, private, never shared with a worktree), `nice -n 10`, running `verify-tree.py --mode archive` for one candidate commit at a time and writing a receipt next to the lane receipts. | | |
| TASK-037 | Either option: `CONTRIBUTING.md` and `README.md` gate lists become `test-lanes.py --lane fast` during development, `--lane heavy` when the fast lane exits 3, and the full gate once per candidate commit via the selected path. Remove the per-task `verify-tree.py --mode archive` instruction. | | |
| TASK-038 | Either option: a Linux run of the default-member test set must pass. Record the first green run (workflow URL for A, receipt path for B) in §6 TEST-008. Failures caused by macOS-only assumptions (keychain, paths) are fixed in this task, not skipped. | | |

### Implementation Phase 8: documentation and re-recorded timings (FV-09)

- GOAL-008: Contributor documentation, the lane script's own docs and the recorded timings describe the new gate set truthfully.

| Task | Description | Completed | Date |
|------|-------------|-----------|------|
| TASK-039 | Re-record `scripts/test-timings.json` from one full nextest run (`--record-timings` on the JUnit file). Confirm no target other than `kontor-tests-e2e` exceeds the 20 s threshold; list any that does in §7 as a residual. | | |
| TASK-040 | Update the `scripts/test-lanes.py` module docstring, `CONTRIBUTING.md` "Tests and evidence", `README.md` and `_docs/index.md` (this plan's router row and the policy row). | | |
| TASK-041 | Add one paragraph to `../../../_docs/ai-orchestration/index.md` "Kontor repository" section pointing seat instructions at the lane commands, so fleet plans stop prescribing `cargo test --workspace` per task. | | |

### Implementation Phase 9: mutation validation (FV-10)

- GOAL-009: Every runtime change in this plan is proven to be tested by a seeded defect that turns the suite red.

| Task | Description | Completed | Date |
|------|-------------|-----------|------|
| TASK-042 | Run every `MUT-` row in §6 that is still `PENDING` against the merged delivery branch, one mutant at a time, reverting between runs. Record KILLED/SURVIVED/EQUIVALENT. | | |
| TASK-043 | For every SURVIVED mutant, strengthen or add the test in the same phase's files, re-run, and update the row. The plan cannot reach `Completed` with a SURVIVED row unaddressed. | | |
| TASK-044 | Configuration mutant MUT-008: remove `LIBSQLITE3_FLAGS` from `.cargo/config.toml`, rebuild, run `test-lanes.py --lane fast --changed crates/kontor-store/src/lib.rs`; the run must report `schema_v1` above the heavy threshold. Restore the file. | | |
| TASK-045 | Final measurement against REQ-001 to REQ-004; set `status: Completed`, then compact this plan per `document-organization` §8 into `_docs/history/`. | | |

### 2.5 Dependency Graph and Waves

#### (a) Plan-Graph Wave diagram

```text
wave 0   FV-01     FV-02     FV-04     FV-06     FV-07     FV-08(decision)   ← 6 concurrent
           │         │         │  └────┐                     │
wave 1     │       FV-03 ◀─────┘       FV-05                 │              ← 2
           │         │                   │                   │
wave 2     └───────▶ FV-09 ◀─────────────┴───────────────────┘              ← 1
                     │
wave 3               FV-10                                                  ← 1
```

Cross-stream joins, each with its reason:

- **FV-03 joins FV-02 and FV-04.** The nextest commands must reflect default members (FV-02), and process-per-test only pays off once the template is shared across processes (FV-04); before that each nextest process would migrate its own template.
- **FV-05 joins FV-04.** Both edit `crates/kontor-store/tests/support/mod.rs`; consolidating binaries after the template rewrite avoids a second pass over the same file.
- **FV-09 joins FV-03, FV-05 and FV-08.** Documentation and re-recorded timings describe the final commands, binaries and gate path; writing them earlier produces text that must be rewritten.
- **FV-10 joins everything.** Mutation validation runs over the merged change set.

#### (b) Kontor/Jira binding tree

There is no Kontor epic or task for this work by operator decision; the "Kontor id" column is therefore `none` for every node, and this is the recorded reason.

```text
ASMA-8258  Kontor fast verification: per-task tests under two minutes     Jira Tech task · Kontor id: none (bypassed)
  ├── FV-01  SQLite build flags                          Phase 1 · wave 0 · standard
  ├── FV-02  Desktop crate out of the default gate        Phase 1 · wave 0 · standard
  ├── FV-04  Shared on-disk template fixture              Phase 2 · wave 0 · standard
  ├── FV-06  Injectable timeouts                          Phase 5 · wave 0 · standard
  ├── FV-07  Machine hygiene                              Phase 6 · wave 0 · standard
  ├── FV-08  Verification policy                          Phase 7 · wave 0 · HIGH-STAKES · decision-gated
  ├── FV-03  nextest lane runner                          Phase 3 · wave 1 · standard
  ├── FV-05  One integration binary per crate             Phase 4 · wave 1 · standard
  ├── FV-09  Documentation and timings                    Phase 8 · wave 2 · standard
  └── FV-10  Mutation validation                          Phase 9 · wave 3 · standard
```

All ten nodes are children of the single Jira task; the work-item code and the confirmed Jira key appear together above, which is the join a reader needs.

#### (c) The scheduling facts

- **Maximum useful concurrency:** 6 in wave 0, then 2, 1, 1.
- **Concurrency cap authorised:** 1. Igor runs one delivery team by hand; the graph's width is recorded so a later run can widen it, not so this run does.
- **Recommended start:** FV-01, because it is two lines and every later measurement depends on it. Serial order for the single team: FV-01, FV-02, FV-04, FV-03, FV-06, FV-05, FV-07, FV-08 (after the decision), FV-09, FV-10.
- **Intra-phase parallelism:** none used. Inside Phase 1, TASK-001 to TASK-003 and TASK-004 to TASK-007 are independent; inside Phase 2, TASK-008 precedes TASK-009.
- **Collision control:** `scripts/test-lanes.py`, `Cargo.toml` and the store test files are edited by other in-flight Kontor branches. The delivery branch rebases on `origin/master` before every phase commit; FV-05 (mass file moves) is deliberately late so it collides with as few open test edits as possible and merges within a day of landing. No shared writable `target/`.
- **External fences:** FV-08 is fenced by Igor's written decision in Phase 7, not by any sibling; it reads `ready` in the graph and must not start on that basis. Option A additionally depends on repository settings only Igor can change (merge queue, required check).
- **Paseo placement:** one Paseo workspace named `TSW • ASMA-8258` rooted at the delivery worktree, one persistent delivery seat titled by its role code only. Providers available on this machine are `claude-work`, `codex-work`, `cursor`, `deepseek-harness` and `opencode`; pick per the fleet model policy. Read back `projectId`, `workspaceId` and `cwd` after creation.

## 3. Alternatives

- **ALT-001: Convert the four suites to template fixtures first (committee Phase 2, three to four days).** Rejected as the first step: the flags in FV-01 reach 7.7 s for `schema_v1` without touching a test; templates remain valuable and arrive with FV-04 in a form nextest can share.
- **ALT-002: Content-hash receipts that let seats skip tests (committee Phase 4).** Rejected: it adds a trust model, signing and an inventory key to avoid work that, after FV-01 to FV-05, costs under two minutes.
- **ALT-003: One shared writable `target/` across worktrees.** Rejected: cargo keys workspace crates by path, so sharing gains nothing for them and risks serving another worktree's artifact.
- **ALT-004: sccache as the first lever.** Deferred to FV-07 with an A/B, because it cannot hit across worktrees without base directories and cannot cache the final test-binary links at all.
- **ALT-005: Delete or `#[ignore]` slow tests.** Rejected by REQ-005; the slow tests were slow because of the build, not their content.
- **ALT-006: RAM disk or `synchronous = OFF` for test databases.** Rejected: it changes what the store tests exercise, and the measured cost was lock contention, not I/O.
- **ALT-007: Keep the no-CI policy and add a local broker.** Kept as FV-08 option B, because it still leaves one Mac doing every full gate; option A is recommended.

## 4. Dependencies

- **DEP-001**: `cargo-nextest` binary on the operator Mac (and in CI for option A).
- **DEP-002**: `sccache` 0.18.0 with `SCCACHE_BASEDIRS` support (already installed).
- **DEP-003**: Workspace crates `sha2`, `hex` and `fs4` already pinned in `Cargo.toml`; no lockfile change.
- **DEP-004**: For option A, GitHub Actions on `Carasent-ASMA/asma-rs-kontor` and the merge-queue repository setting.
- **DEP-005**: Igor's decision line in Phase 7.

## 5. Files

- **FILE-001**: `.cargo/config.toml` (new): `LIBSQLITE3_FLAGS`.
- **FILE-002**: `Cargo.toml`: `default-members`, `[profile.dev.package.libsqlite3-sys]`.
- **FILE-003**: `.config/nextest.toml` (new).
- **FILE-004**: `scripts/test-lanes.py`, `scripts/verify-tree.py`, `scripts/test-timings.json`, `scripts/tests/test_lanes.py` (new), `scripts/jobserver.sh` (new), `scripts/prune-worktree-targets.py` (new), `scripts/full-gate-queue.sh` (new, option B).
- **FILE-005**: `crates/kontor-store/src/migrations.rs` (`schema_fingerprint`, `OpenOptions`, `open_with`), `crates/kontor-store/src/lib.rs` (re-exports, `test_support` module), `crates/kontor-store/Cargo.toml` (feature, `autotests`, `[[test]]`).
- **FILE-006**: `crates/kontor-store/tests/support/mod.rs`, `crates/kontor-store/tests/main.rs` (new), every `crates/kontor-store/tests/*.rs` (becomes a module), `crates/kontor-store/tests/schema_v1.rs` (busy-writer test).
- **FILE-007**: `crates/kontor-core/tests/main.rs` (new), `crates/kontor-core/Cargo.toml`.
- **FILE-008**: `crates/kontor-daemon/tests/harness/mod.rs`, `crates/kontor-daemon/Cargo.toml` (dev-dependency feature).
- **FILE-009**: `crates/kontor-jira/src/*` (request timeout parameter), `crates/kontor-jira/tests/native_connector.rs`.
- **FILE-010**: `CONTRIBUTING.md`, `README.md`, `docs/DEVELOPMENT-MACHINE.md` (new), `_docs/index.md`, `_docs/architecture/2026-09-01-13-25-architecture-kontor-local-verification-policy.md` (superseded under option A), new architecture decision (option A), `.github/workflows/verify.yml` (new, option A).
- **FILE-011**: `../../../_docs/ai-orchestration/index.md` (one paragraph).
- **FILE-012**: `~/.cargo/config.toml` on the operator Mac (not versioned).

## 6. Testing

- **TEST-001**: Per-suite wall/user/sys before and after FV-01 for the five suites in the introduction table plus `account_security`; pass per REQ-003.
- **TEST-002**: Unit count of a warm `cargo test --no-run` before and after `default-members`; pass when `kontor-desktop` and its 330 packages are absent.
- **TEST-003**: TASK-011 template-cache tests; `cargo nextest run -p kontor-store` twice in a row shows the second run reusing the cached template (no migration log line).
- **TEST-004**: `scripts/tests/test_lanes.py` passes; a fast-lane receipt on a store-only change lists per-test timings and finishes under 120 s (REQ-001).
- **TEST-005**: `cargo nextest list` counts unchanged by FV-05; link seconds per test target from `cargo build --timings` before and after.
- **TEST-006**: The busy-writer and Jira timeout tests finish under 2 s each and still assert the timeout outcome.
- **TEST-007**: sccache cross-worktree A/B hit counts; jobserver concurrent `rustc` count at or below ten.
- **TEST-008**: First green Linux full-gate run (option A workflow URL or option B receipt).
- **TEST-009**: Full workspace nextest run under 600 s (REQ-002) recorded in `scripts/test-timings.json`.

`PENDING` in the Result column means the mutant has not been run yet; every row must read KILLED or EQUIVALENT before `status: Completed`.

| Mutant | File:line | Suite | Result | Action |
|--------|-----------|-------|--------|--------|
| **MUT-001**: skip the template `integrity_check` | `crates/kontor-store/src/test_support.rs` (new) | `store` (TASK-011 b) | PENDING | |
| **MUT-002**: return the `.building` path before rename | `crates/kontor-store/src/test_support.rs` (new) | `store` (TASK-011 a, c) | PENDING | |
| **MUT-003**: fingerprint hashes only `SCHEMA_VERSION` | `crates/kontor-store/src/migrations.rs` (`schema_fingerprint`) | `kontor-store` unit (TASK-008) | PENDING | |
| **MUT-004**: JUnit reader ignores `<failure>` | `scripts/test-lanes.py` (`parse_junit`) | `scripts/tests/test_lanes.py` | PENDING | |
| **MUT-005**: desktop flag never set | `scripts/test-lanes.py` (`classify`) | `scripts/tests/test_lanes.py` | PENDING | |
| **MUT-006**: busy timeout option ignored | `crates/kontor-store/src/migrations.rs` (`configure_connection`) | `store` (`a_busy_writer_waits_then_times_out_without_partial_state`) | PENDING | |
| **MUT-007**: request timeout option ignored | `crates/kontor-jira/src/` (connector constructor) | `native_connector` | PENDING | |
| **MUT-008**: `LIBSQLITE3_FLAGS` removed | `.cargo/config.toml` | lane heavy-threshold check (TASK-044) | PENDING | |

## 7. Risks & Assumptions

- **RISK-001**: `SQLITE_DEFAULT_MEMSTATUS=0` also applies to the production daemon binary built from this tree. It disables `sqlite3_memory_used` statistics and the `SQLITE_MAX_MEMORY` limit; the workspace uses neither (grep confirmed 2026-09-25). It is on SQLite's own recommended list.
- **RISK-002**: Consolidating integration binaries can surface duplicate private helper names across files. Mitigation: rename helpers only, never tests, and prove counts equal (TASK-024).
- **RISK-003**: nextest process-per-test changes test isolation assumptions: a test relying on another test's in-process side effect will start failing. That is a defect surfaced, not created; fix the test.
- **RISK-004**: The shared template cache under `target/tmp` is per worktree, so the first run in a new worktree still pays one migration. Acceptable at 0.7 s.
- **RISK-005**: `SCCACHE_BASEDIRS` on 0.18.0 has a reported wrong-object bug in preprocessor cache mode; `SCCACHE_DIRECT=false` avoids it, and TASK-031 keeps the setting only with observed hits.
- **RISK-006**: Cargo may not honour a FIFO jobserver in the exact form used; TASK-032 measures, and the fallback is `CARGO_BUILD_JOBS` per seat.
- **RISK-007**: Option A runs on Linux; macOS-only assumptions in tests (keychain, temp paths) will fail there and must be fixed under TASK-038.
- **RISK-008**: Other in-flight branches edit the same test files; late merge of FV-05 and daily rebases limit conflicts but do not remove them.
- **ASSUMPTION-001**: The operator Mac keeps 10 cores and roughly 300 GB free; measurements in §6 are taken with other seats running, so wall times will vary and the pass conditions include headroom.
- **ASSUMPTION-002**: The `-D warnings` clippy gate applies to every new code path in this plan; no `allow` attributes are introduced.
- **ASSUMPTION-003**: The team works one phase per turn and does not start Phase 7 before the decision line exists.

## 8. Related Specifications / Further Reading

- [Local verification policy (2026-09-01)](../architecture/2026-09-01-13-25-architecture-kontor-local-verification-policy.md), superseded under FV-08 option A.
- [Jira ASMA-8258](https://carasent.atlassian.net/browse/ASMA-8258).
- Commit `65fe8af8`: test lanes, stable lock check, migrated template for the store suite.
- [SQLite compile-time options, recommended list](https://www.sqlite.org/compile.html).
- [cargo-nextest: how it works](https://nexte.st/docs/design/how-it-works/), [setup scripts](https://nexte.st/docs/configuration/setup-scripts/), [archiving builds](https://nexte.st/docs/ci-features/archiving/).
- [Delete Cargo Integration Tests (matklad)](https://matklad.github.io/2021/02/27/delete-cargo-integration-tests.html).
- [sccache configuration, base directories](https://github.com/mozilla/sccache/blob/main/docs/Configuration.md).
- [rustc jobserver](https://doc.rust-lang.org/rustc/jobserver.html).
- [GitHub merge queue](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue).
