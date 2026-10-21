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
  labels in the changed test fixtures are synthetic. The two Teams source
  fixtures keep their `38`/`36` binding counts; their hash pins now cover the
  scrubbed bytes.

## Post-merge integration

Merged `origin/master` at `f1223221` (`535d1be5` #413, `f1223221` #418) with
no rebase or force. The merge had no textual conflicts. The merged ASMA-8550
Teams source tests pin the exact fixture bytes, so their expected SHA-256 for
`authoring-fleet.yml` and `operator-fleet.yml` was updated to the scrubbed
bytes; the fleet and runtime behaviour, the alias-exemption constant and the
51-file fence are unchanged.

## Provenance and migration

The committed projection `.github/kontor-project.yml` is generated from the
named ASMA schema fixture `tests/fixtures/config/asma-orchestration.yml`, not
from the live asma-modules config (that config does not yet carry a `project:`
block). Its `generated` block records the repo-relative source, a
`source_revision` of `sha256:<digest>` and the same `source_sha256`. It carries
two check runs from the fixture, including the publication check selected
below. The projection maps those values into the trusted-base workflow:

| Project key | Repository variable | Projection field |
| --- | --- | --- |
| `project.key` | `KONTOR_PROJECT_KEY` | `project.key` |
| `project.forge.base_branch` | `KONTOR_BASE_BRANCH` | `project.forge.base_branch` |
| the `project.check_runs` entry ending `publication-branch-title` | `KONTOR_CHECK_NAME` | `project.check_runs` |

When all three repository variables are set, CI writes a temporary projection
whose `generated.source` is `repository-variables` and whose
`generated.source_revision` is `github.workflow_sha`. When none is set, CI
reads the committed projection. Setting only some of them fails closed.
Regenerate the committed projection exactly with:

```sh
python3 scripts/kontor_project.py \
  --config-file tests/fixtures/config/asma-orchestration.yml \
  project --format yaml > .github/kontor-project.yml
```

## Review corrections

- **Configured check name.** The workflow no longer prints the configured name
  only in the checker record. It opens a check run through the Checks API with
  the name read from the selected projection, then completes that check run
  with the job result. The job's own check name is the neutral workflow label
  `Publication branch and title identity`; branch protection must require the
  configured name. The workflow gains `checks: write`.
- **Regeneration provenance.** `scripts/kontor_project.py project --format
  yaml` now serializes the complete projection, including `generated:`. A
  source inside this checkout records its repo-relative path and
  `source_revision: sha256:<digest>` plus `source_sha256`; a source outside it
  uses the public-safe labels. JSON output carries the same provenance. The
  committed projection round-trip is tested against its named fixture.
- **Quoted strings.** Both generators quote every string. A config with
  `key: "ON"` or `base_branch: "2026"` round-trips as strings instead of
  reloading as a boolean or integer.
- **Preflight failure.** The configured check is opened before repository
  variables are validated. A partial or invalid variable set creates the
  committed projection's check and completes it as failure, so an earlier
  success on the same head SHA (for example after a title edit) cannot remain
  the required result. A regression covers previous success, partial variables
  and a title edit.
- **Pre-push selection.** In a clean agent shell the hook falls back to
  `.github/kontor-project.yml`; an explicit `KONTOR_CONFIG_DIR` or
  `KONTOR_CONFIG_FILE` still wins. The clean-environment path is tested.
- **Evidence restoration.** The four `docs/evidence/ASMA-8454/archive-failure`
  scripts and the dated desk-qualification report were restored to
  `origin/master`; the moved qualification runner remains at
  `evidence/ASMA-8454-qualify-durable-desks.py`. The present non-redacted
  manifest pins are 90/90 (`cleanup-receipt.json` is absent from the bundle).
- **Non-blocking notes.** N2: the tokenomics store-label row is tagged WP-08.
  N3: `apps/paseo-plugin/client/page-data.ts` still keys Jira links to the ASMA
  tenant; it has no project-config surface in the plugin client, so it is
  recorded for WP-08/WP-12 follow-up. N4: `test-lanes.py --lane full` no longer
  reads a project selection, and the README/CONTRIBUTING examples show the
  selection for the fast/heavy lanes. N8: the dated report body was restored.
  N9: a CHANGELOG note records the configuration, hook and check-name changes.
  N5/N6/N7 remain recorded reviewer notes with no WP-12 code change.

## Blocked rows

| Row | Owner | Why it is blocked here |
| --- | --- | --- |
| `apps/console/public/tokenomics/index.html` store labels (`STORE_LABEL`) still key on the deployment's provider-account aliases | WP-08 (re-tagged from WP-12) | The file is one of the 51 `ALIAS_CODE_EXEMPTION` files. The alias scrub and the exemption removal are WP-08; WP-12 changes only the payload Jira base. |
| Any remaining provider-account alias literal in the 51 `ALIAS_CODE_EXEMPTION` files | WP-08 | Same fence: WP-12 did not rename or remove those files, edit the exemption constant, or claim the tree alias-free. |
| Person names in `crates/kontor-daemon/src/applications/desk_adams.rs` runtime strings | WP-08 | WP-08 owns runtime strings; WP-12 scrubbed tests and fixtures only. |
| `apps/paseo-plugin/client/page-data.ts` hardcodes the ASMA key pattern and Jira tenant | WP-12 follow-up | The plugin client has no project-config surface; recorded rather than silently changed. |

## Verified

```sh
python3 scripts/public_tree.py .
python3 -m unittest discover -s scripts/tests
python3 -m unittest discover -s .github/scripts/tests
pnpm --filter @carasent/kontor-paseo-plugin test
pnpm --filter kontor-console typecheck
pnpm --filter kontor-console test
pnpm --filter kontor-console build
pnpm -r typecheck
cargo fmt --all -- --check
cargo test -p kontor-fleet --lib --locked
cargo test -p kontor-accounts --locked
cargo test -p kontor-core --locked
cargo test -p kontor-store --test durable_seat_declarations --locked
cargo test -p kontor-runtime-paseo --locked
cargo test -p kontor-daemon --lib --locked
cargo test -p kontor-daemon --test loopback_api durable_seats:: --locked
cargo test -p kontor-daemon --test loopback_api desk_adams:: --locked
cargo test -p kontor-daemon --test loopback_api pr_reviews:: --locked
cargo test -p kontor-daemon --test loopback_api qualification:: --locked
cargo test -p kontor-daemon --test loopback_api a_configured_jira_boundary_distinguishes_historical_from_native_completion --locked -- --exact --ignored
```

The whole-tree public check reports no home path or run receipt and no account
alias outside the 51 exempt files. The console reports 306 passing tests and a
successful production build; the Paseo plugin reports 1,133 passing tests after
the merge; the scripts report 263 passing/1 skipped and the publication tests
38 passing after the review corrections. The two `qualification::` loopback
tests pass on the merged head.

`python3 scripts/verify-tree.py --mode inplace` was started, but it did not
finish inside the 40-minute session limit. Three `kontor-daemon` loopback tests
failed independently of this change: two report
`unsupported_capability` from the runtime quota-provenance fixture
(`a_configured_governed_summarizer_produces_the_persisted_handoff_summary`,
`startup_composes_the_configured_runtime_summarizer`) and one exceeded its
resident-controller wait
(`resident_failed_epic_apply_emits_no_immediate_replay_wake`). All three fail
identically at the new base `f1223221` in a detached worktree, and on the
merged tree, so they remain inherited baseline failures, not WP-12
regressions. `a_configured_jira_boundary_distinguishes_historical_from_native_completion`
is marked ignored as superseded; forced with `--ignored` it fails identically
on the merged head and at `f1223221`. The full gate was not rerun after the
merge; targeted coverage is the command list above.
