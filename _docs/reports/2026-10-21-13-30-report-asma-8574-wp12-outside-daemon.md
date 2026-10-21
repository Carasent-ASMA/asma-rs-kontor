# ASMA-8574 — WP-12 outside the daemon

> **Date:** 2026-10-21 13:30 Europe/Oslo
> **Status:** Implemented; awaiting review
> **Category:** report
> **Scope:** ASMA-8574 / WP-12; console, publication check, hooks, local scripts and test fixtures
> **Summary:** The console reads the live model catalog and ships an obviously fake offline fixture; the publication check, pre-push hook and test lanes read the project key, base branch and check name from the selected YAML config folder; the branch grammar is read from `kontor-core`; the one-off desk qualification runner moved to evidence.

---

## What changed

- **Console.** `apps/console/src/state/teams.ts` now offers a synthetic
  `alpha`/`beta`/`gamma` offline fixture with invented routes and no real
  provider or model id. Production still reads `GET /v1/catalog` through
  `KontorClient.modelCatalog()`. The telemetry citation no longer carries a
  home path, and the connection placeholder uses the real default port `7717`.
- **Tokenomics.** The Jira browse base now comes from the payload's
  `jiraBase`; there is no tenant fallback.
- **Publication check.** `.github/scripts/publication_branch_title.py` reads
  `project.key`, `project.forge.base_branch` and the publication entry in
  `project.check_runs`. The trusted-base workflow fetches the checker, the
  project helper, the YAML reader, `crates/kontor-core/src/branch.rs` and the
  committed projection from the exact workflow revision. Repository variables
  are projected into a generated YAML file whose `generated:` block records
  `source: repository-variables`, the workflow revision and the variable names.
- **Pre-push hook.** `.githooks/pre-push` calls
  `scripts/kontor_project.py`; it keeps no project key, base branch or branch
  grammar of its own. `KONTOR_ALLOW_AGENT_PUSH=1` replaces the retired
  `ASMA_ALLOW_AGENT_PUSH` escape hatch.
- **Test lanes.** `scripts/test-lanes.py` reads the project configuration,
  derives `origin/<base_branch>` and `<base_branch>` as its default diff bases,
  and records the resolved project values in its receipt.
- **Evidence move.** `scripts/qualify-durable-desks.py` moved to
  `evidence/ASMA-8454-qualify-durable-desks.py`; its runbook and report
  references were updated.
- **Tests.** Owner names, the owner email, home paths and account display
  labels in the changed test fixtures are synthetic.

## Provenance and migration

The committed projection `.github/kontor-project.yml` maps the selected config
folder's `orchestration/orchestration.yml` values into the trusted-base
workflow:

| Project key | Repository variable | Projection field |
| --- | --- | --- |
| `project.key` | `KONTOR_PROJECT_KEY` | `project.key` |
| `project.forge.base_branch` | `KONTOR_BASE_BRANCH` | `project.forge.base_branch` |
| the `project.check_runs` entry ending `publication-branch-title` | `KONTOR_CHECK_NAME` | `project.check_runs` |

When all three repository variables are set, CI writes a temporary projection
whose `generated.source` is `repository-variables` and whose
`generated.source_revision` is `github.workflow_sha`. When none is set, CI
reads the committed projection. Setting only some of them fails closed.
Regenerate the committed projection with:

```sh
python3 scripts/kontor_project.py --config-dir <config folder> project --format yaml
```

## Blocked rows

| Row | Owner | Why it is blocked here |
| --- | --- | --- |
| `apps/console/public/tokenomics/index.html` store labels (`STORE_LABEL`) still key on the deployment's provider-account aliases | WP-08 | The file is one of the 51 `ALIAS_CODE_EXEMPTION` files. The alias scrub and the exemption removal are WP-08; WP-12 changes only the payload Jira base. |
| Any remaining provider-account alias literal in the 51 `ALIAS_CODE_EXEMPTION` files | WP-08 | Same fence: WP-12 did not rename or remove those files, edit the exemption constant, or claim the tree alias-free. |
| Person names in `crates/kontor-daemon/src/applications/desk_adams.rs` runtime strings | WP-08 | WP-08 owns runtime strings; WP-12 scrubbed tests and fixtures only. |

## Verified

```sh
python3 scripts/public_tree.py .
python3 -m unittest discover -s scripts/tests
python3 -m unittest discover -s .github/scripts/tests
pnpm --filter kontor-console typecheck
pnpm --filter kontor-console test
pnpm --filter kontor-console build
pnpm -r typecheck
cargo fmt --all -- --check
cargo test -p kontor-fleet --lib --locked
cargo test -p kontor-accounts --locked
cargo test -p kontor-core --locked
cargo test -p kontor-store --test durable_seat_declarations --locked
cargo test -p kontor-daemon --test loopback_api durable_seats:: --locked
cargo test -p kontor-daemon --test loopback_api desk_adams:: --locked
cargo test -p kontor-daemon --test loopback_api pr_reviews:: --locked
```

The whole-tree public check reports no home path or run receipt and no account
alias outside the 51 exempt files. The console reports 306 passing tests and a
successful production build.

`python3 scripts/verify-tree.py --mode inplace` was started, but it did not
finish inside the 40-minute session limit. Three `kontor-daemon` loopback tests
failed independently of this change: two report
`unsupported_capability` from the runtime quota-provenance fixture
(`a_configured_governed_summarizer_produces_the_persisted_handoff_summary`,
`startup_composes_the_configured_runtime_summarizer`) and one exceeded its
resident-controller wait
(`resident_failed_epic_apply_emits_no_immediate_replay_wake`). All three fail
identically at base commit `3bcbfe95` in a detached worktree, so they are
inherited baseline failures, not WP-12 regressions.
