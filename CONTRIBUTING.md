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

The delivery method is [Workflow](docs/WORKFLOW.md), the vocabulary is
[Glossary](docs/GLOSSARY.md), and the governing principles are
[Principles](docs/PRINCIPLES.md). Kontor decisions are recorded in
[`docs/decisions/`](docs/decisions/README.md).

### Product availability

Follow the canonical workspace [Kontor product availability decision
(Igor, 2026-10-23)](https://github.com/Carasent-ASMA/asma-modules/blob/master/.github/instructions/memory.instructions.md#kontor-product-availability):
shipped, supported features, including **New LSA**, are standard and available
by default, without per-feature opt-in or experimental/lab enablement.
**Desk PR is the sole opt-in exception:** ask Igor for explicit enablement.
Feature owners apply that contract through normal review/release, preserving
permissions, real host capability guards, explicit operator-off safeguards and
action intent; unsupported or deferred functionality stays so.

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
in `Cargo.toml`): backtraces keep file and line, a debugger loses variable
inspection. The setting exists because parallel worktrees each keep a
`target/` of their own.

## Work in one focused branch

- Name the branch `<type>/<KEY>-<number>-<slug>` from the confirmed tracker key
  (`feat`, `fix`, `docs`, `chore`, `refactor`, `test`, `perf`, `build`, `ci`,
  `releases`); the only keyless form is a hotpatch line
  `releases/v<major>.<minor>.<patch>`.
  [Native naming](docs/NATIVE_NAMING.md) describes the same grammar Kontor
  derives for task worktrees (ASMA-8101). Enable the local preflight once per
  clone with `git config core.hooksPath .githooks`; `.githooks/pre-push`
  refuses an agent push of a non-canonical branch or of the configured base
  branch before the network round trip (it recognizes agent shells by their
  environment; humans push normally).
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
and it builds and tests nothing. Igor's **2026-10-25 decision** requires tests
for changed/added behavior and specifically affected callers/contracts, with no
automatic full repository/workspace/crate/package/plugin suites. The governing
[local verification policy](_docs/architecture/2026-09-01-13-25-architecture-kontor-local-verification-policy.md)
amends DEC-002; missing GitHub checks are not a waiver.

Before running checks, record an impact plan at the exact candidate head/tree
and base. Trace imports/callers and contracts, including non-plugin consumers
of shared plugin modules. Package hints alone are insufficient. Keep relevant
scoped build/typecheck/format, dependency/lock/security, independent QA/AUD,
affected browser/UI and installed-readback obligations. Runtime libraries
bundled from `devDependencies` remain security scope. Rust executor/contract
changes still require all affected behavior and actual adapter/caller proof,
not an automatic crate/workspace suite.

The existing verifier executes explicit argv commands without a shell:

```sh
python3 scripts/verify-tree.py --mode archive --impact /tmp/impact.json --receipt /tmp/verification.json
```

`archive` extracts `git archive HEAD` without untracked files or local overlays.
`staged` exports the index; `inplace` requires unchanged tracked files. Those
last two modes are development evidence only. All modes check the public-tree
boundary, then only the commands selected in the plan. There is no implicit
Cargo workspace test, recursive pnpm test, whole-plugin suite or lock refresh.

The JSON plan uses this shape (replace identities and selection with the actual
reviewed diff; these commands illustrate a Python-tooling-only change):

```json
{
  "head": "<exact candidate commit>",
  "tree": "<exact candidate Git tree>",
  "base": "<exact base commit>",
  "impact_analysis": "Trace changed behavior, direct callers and contracts; explain cross-package exclusions.",
  "impact": [
    {
      "path": "scripts/verify-tree.py",
      "reason": "Changed verifier; list every other changed path and affected caller in additional rows.",
      "tests": ["scripts/tests/test_verify_tree.py"]
    }
  ],
  "checks": [
    {
      "kind": "tests",
      "cwd": ".",
      "argv": ["python3", "-B", "-m", "unittest", "discover", "-s", "scripts/tests", "-p", "test_verify_tree.py"],
      "reason": "Focused verifier behavior; no other tooling changed.",
      "tests": ["scripts/tests/test_verify_tree.py"]
    }
  ],
  "not_applicable": {
    "build": "Python-only change; no compiled artifact changes.",
    "typecheck": "No typed runtime or contract changes.",
    "format": "Replace with actual scoped formatting evidence or an exact reason.",
    "dependencies": "No manifest/lock/dependency or release-graph change.",
    "security": "Replace with changed-tooling security evidence or an exact impact-based reason."
  },
  "external": {
    "qa": "Required: exact-head independent QA receipt, pending here.",
    "aud": "Required: exact-head independent AUD receipt, pending here.",
    "browser": "No UI change in this illustrative tooling diff.",
    "installed": "No runtime artifact changed in this illustrative tooling diff."
  },
  "prior_receipts": []
}
```

All changed/deleted paths must appear in `impact`; include affected unchanged
callers with the selected test names too. Use relevant cases (`-t`, Rust exact
filters, or explicit unittest names) in mixed-purpose files. Whole focused new
files are valid. `checks.kind` is `tests`, `build`, `typecheck`, `format`,
`dependencies` or `security`. Each absent kind requires an exact
`not_applicable` reason. These reasons are reviewed, not an automatic waiver.
A relevant blocked/failed check remains unresolved, never inapplicable.

`external` records separate review/runtime obligations, not automatic approval.
Reuse prior evidence via `prior_receipts` rows with `path`, `sha256` and its
unchanged result/disposition. Failed historical receipts stay FAIL, including
failed broad wrappers; diagnostic passes do not replace them. The verifier
refuses existing receipt/log paths and records selected results and remaining
unrun commands. Its zero exit covers local selected checks only. No retry loop,
ignore, timeout relaxation or security waiver is implied.

The existing package/dependent planner remains available for **hints only**:

```sh
KONTOR_CONFIG_FILE=.github/kontor-project.yml python3 scripts/test-lanes.py --lane fast --dry-run
python3 scripts/test-lanes.py --lane fast --mode archive --impact /tmp/impact.json --receipt /tmp/verification.json
```

Legacy `fast`, `heavy` and `full` execution all delegate to the same explicit
impact plan; none expands to package suites. `--record-timings <log>` only
imports a retained log and launches no tests. Logs/receipts use
`target/test-lanes/` by default. Use a new receipt path for every attempt.

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

After committing, use the impact-selected `--mode archive` command above.
The head, Git tree and exact base diff must match the plan. A changed head or
impact requires updated coverage/evidence; do not transfer a previous PASS to a
different candidate. Assess lock completeness with the applicable locked/frozen
command in the plan, not a fresh dependency resolution.

## The selected project configuration

The publication check, the `pre-push` hook and `scripts/test-lanes.py` read the
project key, base branch and check name from one explicitly selected YAML
config folder (`KONTOR_CONFIG_DIR`, or `--config-dir` where the tool offers
it). The branch vocabulary comes from
`crates/kontor-core/src/branch.rs`; none of those tools keeps its own copy.
There is no implicit fallback for the tools. The `pre-push` hook alone falls
back to the committed projection when neither selection is set, so a clean
agent shell can push this repository.

The trusted-base workflow cannot read an external folder. It prefers the
repository variables `KONTOR_PROJECT_KEY`, `KONTOR_BASE_BRANCH` and
`KONTOR_CHECK_NAME` when all three are set, and writes a temporary projection
recording them as the source. Otherwise it reads
[`.github/kontor-project.yml`](.github/kontor-project.yml). That committed
projection is generated from
[`tests/fixtures/config/asma-orchestration.yml`](tests/fixtures/config/asma-orchestration.yml),
the ASMA schema test fixture. The live asma-modules config does not yet carry a
`project:` block; do not treat it as the projection source. Regenerate the
projection exactly with:

```sh
python3 scripts/kontor_project.py \
  --config-file tests/fixtures/config/asma-orchestration.yml \
  project --format yaml > .github/kontor-project.yml
```

The generated YAML carries the complete `generated:` provenance and quotes
every string, so values such as `ON` or `2026` stay strings on reload. The
workflow opens an API check run whose name it reads from the selected
projection and completes it with the job result. Branch protection must require
both that configured check and the static
`Publication branch and title identity` job check: the static check fails on
every preflight or configuration error, including an unresolvable check name.

The retired escape hatch `ASMA_ALLOW_AGENT_PUSH` is now
`KONTOR_ALLOW_AGENT_PUSH`.

## Pull requests

The pull-request title leads with the branch's key and one space
(`<KEY>-<number> <summary>`); a `releases/v<X>.<Y>.<Z>` branch takes exactly
`Release v<X>.<Y>.<Z>`, and the base is the configured
`project.forge.base_branch`. The publication check
(`.github/workflows/publication-branch-title.yml`) reports any other branch,
title or base as a failure, with one exception: a `dependabot/` branch
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
