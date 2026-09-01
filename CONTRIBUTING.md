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

The repository pins Rust, Node/pnpm expectations, Rust dependencies and
JavaScript dependencies. Keep both lockfiles committed and reproducible.

## Work in one focused branch

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

Kontor does not use GitHub Actions CI. Run the gates relevant to your change,
then the complete set locally against the exact candidate commit before
requesting merge. Record the commands and results in the pull request or owning
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

## Pull requests

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
