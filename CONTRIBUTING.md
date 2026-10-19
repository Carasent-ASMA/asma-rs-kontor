# Contributing to ASMA Kontor

Thank you for improving Kontor. The project is pre-1.0, so small, reviewable
changes with explicit evidence are more valuable than broad speculative
frameworks.

By participating, you agree to the [Code of Conduct](CODE_OF_CONDUCT.md). Do
not report vulnerabilities in public issues; follow [Security](SECURITY.md).

## Before writing code

For a non-trivial behavior or architecture change, open or reference an issue
and agree on the acceptance boundary first. This avoids a large implementation
that cannot merge because it creates a second authority, weakens a safety
property or overlaps active work.

Read [Architecture](ARCHITECTURE.md) when changing persistence, scheduling,
policy, commands, runtime adapters, session content, accounts, integrations or
security. The invariants in that document are acceptance criteria, not style
preferences.

## Set up

```sh
git clone https://github.com/Carasent-ASMA/asma-rs-kontor.git
cd asma-rs-kontor
rustup show active-toolchain
pnpm install --frozen-lockfile
```

The repository pins Rust (`rust-toolchain.toml`, 1.97.1), Node/pnpm
expectations (`node >=24`, `pnpm@11.6.0` in `package.json`), Rust dependencies
and JavaScript dependencies. Keep both lockfiles committed and reproducible.

Dev builds carry line tables only (`[profile.dev] debug = "line-tables-only"`
in `Cargo.toml`, PR #232): backtraces keep file and line, a debugger loses
variable inspection. The setting exists because parallel worktrees each keep a
`target/` of their own.

## Work in one focused branch

- Name the branch `<type>/ASMA-<number>-<slug>` from the confirmed Jira key
  (`feat`, `fix`, `docs`, `chore`, `refactor`, `test`, `perf`, `build`, `ci`,
  `releases`); the only keyless form is a hotpatch line
  `releases/v<major>.<minor>.<patch>`.
  [Native naming](docs/NATIVE_NAMING.md) describes the same grammar Kontor
  derives for task worktrees (ASMA-8101). Enable the local preflight once per
  clone with `git config core.hooksPath .githooks`; `.githooks/pre-push`
  refuses an agent push of a non-canonical branch or of `master` before the
  network round trip (it recognizes agent shells by their environment; humans
  push normally).
- Keep one issue/ticket to one reviewable diff.
- Do not mix generated lockfile drift, formatting unrelated files or another
  contributor's work into the change.
- Stage explicit paths; avoid broad staging in a shared checkout.
- Explain user-visible and protocol changes in the pull request.
- Update `README.md`, `ARCHITECTURE.md`, OpenAPI or fixtures when their contract
  changes.

## Design rules

Every change must preserve these boundaries:

1. Kontor writes only Kontor-owned state; adapters use supported external
   interfaces and never edit another tool's internal store.
2. Desired command state, runtime observation and derived safe status remain
   separate.
3. A missing runtime, closed stream or acknowledgement is not a terminal
   verdict.
4. Runtime-changing intent is durable before dispatch and replay is
   idempotent.
5. One team-run role slot has at most one non-terminal native session.
6. Secrets remain references and do not enter databases, logs, exports,
   command arguments, fixtures or tickets.
7. Core workflow/profile identifiers remain open, versioned data rather than
   product-specific enums or branches.

Prefer extending an existing seam to adding another one. New dependencies must
be justified, pinned exactly in the workspace manifest and checked by both
license and advisory gates.

## Tests and evidence

Kontor does not use GitHub Actions CI; the repository's one workflow is the
publication branch/title check described under [Pull requests](#pull-requests),
and it builds and tests nothing. Run the gates relevant to your change, then
the complete set locally against the exact candidate commit before requesting
merge. Record the commands and results in the pull request or owning
delivery evidence; a GitHub check is not a substitute and its absence is not a
waiver. The governing, explicitly reversible decision is the
[local verification policy](_docs/architecture/2026-09-01-13-25-architecture-kontor-local-verification-policy.md).

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
```

The KON-MVP-18 e2e pilot inside `cargo test --workspace` still writes a run
bundle into `docs/evidence/KON-MVP-18/<run-id>/` on every run; delete it and
never commit it until ASMA-8511 moves that output.

During development, `scripts/test-lanes.py` runs only what a change reaches:

```sh
python3 scripts/test-lanes.py --lane fast --dry-run   # print the selection and commands
python3 scripts/test-lanes.py --lane fast             # fmt, clippy + tests of affected quick packages, touched console checks
python3 scripts/test-lanes.py --lane heavy            # slow packages' tests + console typecheck/tests, quiet, JSON receipt
python3 scripts/test-lanes.py --lane full             # scripts/verify-tree.py --mode inplace
```

- The selection is the files changed since the merge base with
  `origin/master` (`--base <rev>` to change it), plus uncommitted files unless
  `--no-worktree`, plus any `--changed <path>`. A change under a package's
  `src/` or its `Cargo.toml` also selects every workspace package that depends
  on it.
- Heavy packages are `kontor-store`, `kontor-daemon` and `kontor-tests-e2e`,
  plus any package with a test target of 60 s or more in
  `scripts/test-timings.json`; refresh that file from a full `cargo test` log
  with `--record-timings <log>`.
- Exit codes: `0` pass, `1` a gate or test failed, `2` usage or tooling
  failure, `3` a bigger lane is needed — `fast` exits `3` after its own checks
  when the change reaches a heavy package, and before running anything when it
  touches the root `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml` or
  `scripts/` (run `full`).
- Logs and receipts land in `target/test-lanes/` (`--receipt <path>` to choose
  the receipt).

A lane is development feedback, not merge evidence; the complete set above
still runs against the candidate commit.

Store and daemon integration tests open realms cloned from one migrated template
(`crates/kontor-store/tests/support/mod.rs`,
`crates/kontor-daemon/tests/harness/mod.rs`) instead of replaying every
migration per test. Clones share one realm identity: a test that compares two
realms opens at least one with `open_created_realm`, and a test about creation
or migration builds its own state root.

Runtime, scheduler, policy, persistence and security changes require:

- a focused unit or contract test that fails for the defect/change;
- recorded/offline protocol fixtures by default;
- a mutation check demonstrating that the test detects the relevant wrong
  behavior;
- final working-tree and lockfile cleanliness.

Tests must be deterministic and clean up every process and temporary resource.
Live daemon/runtime tests must be explicitly opt-in; ordinary tests must not
depend on network access, provider accounts or a user's runtime state.

After committing, verify the committed export rather than only the working
tree:

```sh
python3 scripts/verify-tree.py --mode archive
```

It extracts `git archive HEAD` into a temporary directory without `.git`,
checks that the committed `Cargo.lock` satisfies every manifest under
`--locked` (it does not regenerate the lock; available updates print as notes),
and runs every gate above except `pnpm --filter kontor-console verify:api`,
which runs separately. `--mode staged` does the same for the staged tree before
a commit exists; `--mode inplace` runs the gates in the current tree.

## Pull requests

The pull-request title leads with the branch's key and one space
(`ASMA-<number> <summary>`); a `releases/v<X>.<Y>.<Z>` branch takes exactly
`Release v<X>.<Y>.<Z>`, and the base is `master`. The
`asma/publication-branch-title` check
(`.github/workflows/publication-branch-title.yml`, PR #280) reports any other
branch, title or base as a failure, with one exception: a `dependabot/` branch
opened by `dependabot[bot]` is accepted as `bot_branch_exempt`, and the base and
head-sha checks still run. It proves shape and key equality only, not the Jira
binding. Agent and ASMA CLI publications are also attested against
Kontor's confirmed Jira binding; see
[Native naming](docs/NATIVE_NAMING.md#publication-attestation).

A pull request should include:

- the problem and why it belongs in Kontor;
- the authority/boundary affected;
- the behavior before and after;
- commands run and results;
- mutation or failure-injection evidence when required;
- any residual risk or deliberately unsupported case.

Reviewers may ask for a split when code, generated artifacts and policy changes
cannot be evaluated independently. A self-reported agent verdict is useful
context but not merge evidence; the diff and reproducible checks are.

## Licensing contributions

The repository is licensed `MIT OR Apache-2.0`. By submitting a contribution,
you represent that you have the right to submit it and agree that it is
licensed under those same terms. This repository does **not** state that
contributors assign their copyright to Carasent ASMA.

Dependency or copied-source additions must retain their own notices and update
`THIRD_PARTY_NOTICES.md` when required.
