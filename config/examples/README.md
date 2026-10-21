# Neutral config folder

This folder is one complete, project-neutral example of the config folder the
Kontor workflow pack selects. Copy it, replace the placeholder values, and pass
its absolute path to every consumer. Nothing here names a real account, host,
person, tracker project or repository.

## Selection

One folder holds the authored settings for one project/user installation. It
may sit anywhere on the machine and have any name.

- `--config-dir <path>` selects the folder and wins over `KONTOR_CONFIG_DIR`.
- A relative input is resolved once, against the working directory of the setup
  invocation, and the resolved absolute path is persisted for later commands and
  service launches.
- There is no implicit fallback to the current directory, a state root or a
  legacy project path. A missing selection, a missing folder or invalid YAML is refused by
  name.
- Paseo-direct and `kontord`-governed work read the same YAML source. Setting up
  the direct option does not require `kontord`.

## Layout

```text
<config folder>/
  orchestration/
    orchestration.yml          entry: schema_version, fleet/core_team selectors, project: block
    orchestration.schema.yml   required-key schema for the project: block
    fleet.yml                  fleet policy, schema_version 2
    route-qualifications.yml   hash-bound route qualification records
    teams/core-team.yml        pinned Core Team roster
    tpm-supervision.yml        autonomous TPM cadence profile
  packs/team.yml               project team/profile overlay
  tracker/jira.yml             tracker connection and credential reference
  forge/github-app.example.yml committed GitHub App placeholder
  host.example.yml             committed host schema example
  host.yml                     this machine only; git-ignored, never committed
  quota-signals.yml            optional vendor refusal wording
  paseo-supervision.yml        sample for <state-root>/supervision.yml, not a project key
  .gitignore                   ignores host.yml
```

`project.supervision` follows plan §1.1 and names
`orchestration/tpm-supervision.yml`, the autonomous TPM cadence profile. The
Paseo watchdog policy is a different file: the daemon reads
`<state-root>/supervision.yml`; `paseo-supervision.yml` here is only the sample
copied to that state-root path. It is not a key in `project:`.

Paths are classified, not resolved uniformly:

- **Config-file references** (`fleet`, `core_team`, `project.team_pack`,
  `project.fleet`, `project.supervision`, `project.tracker.spec`) resolve from
  the selected folder root. They are relative, must stay inside the folder, and
  must exist. `project.supervision` names the TPM cadence profile only; the
  Paseo watchdog policy is `<state-root>/supervision.yml`.
- **Project resource paths** (`documentation_roots`, `gap_ledger`, `glossary`,
  `decision_records_root`) start with a `project.resource_roots` entry. Each
  root maps to a repository in `project.forge.repositories`; the rest resolves
  inside that checkout. Absolute paths and `..` are refused. They never resolve
  from this folder.
- **Host paths** (`host.paths.*`, `host.binaries.*`, `host.project_cli.*`) are
  absolute or PATH-resolved host state.
- **Conventions** such as `worktree_layout` are patterns, not paths.
- **Credential references** name a secret-store entry; they are never values.

## Bootstrap and activated settings

| Class | Examples | When it is read | Effect of an edit |
| --- | --- | --- | --- |
| Bootstrap | folder selection, `host.yml`, state root, endpoints, ports, credential references | before the daemon starts | requires a restart for new placements; invalid input refuses start; active runs keep their resolved-route snapshot |
| Activated policy | fleet policy, orchestration bundle, Core Team roster, tracker specs, team/profile packs, committee presets | through preview → publish → activate, bound by content hash | no live effect until activated; active runs keep their pins |
| Startup-read | `quota-signals.yml`, `<state-root>/supervision.yml` (Paseo watchdog) | at daemon start | validated then; invalid input refuses start |
| Generated projection | a native-format file rendered from YAML | whenever its consumer reads it | never hand-edited; regenerate from its YAML source |
| Test data | fixtures under `tests/fixtures/` | tests only | not deployment configuration |

Authored configuration is YAML only (`.yml` or `.yaml`). Where a runtime or
external tool requires JSON or another native format, that file is a generated
projection with a recorded YAML source and content hash; it is not a second
editable source. Credential values stay in the deployment's secret store; YAML
holds references only.

## Replace before use

- every placeholder account alias in `orchestration/fleet.yml` and
  `host.example.yml`;
- every `native_alias`, harness and route order in `host.example.yml` with this
  host's own mapping, and `project_cli.example` with the project's executable;
- every canonical model id in `orchestration/fleet.yml` and every
  route-native id in `host.example.yml` with ids the runtime catalog accepts;
- the synthetic Core Team catalog identity, content hash and team/committee
  ids;
- the example tracker site and project key in `orchestration/orchestration.yml`
  and `tracker/jira.yml`;
- the organization, repositories, `resource_roots`, base branch,
  documentation roots, gap ledger, glossary, attention channel, worktree
  layout, check-run names, capacity, time zone, holiday countries and currency
  in the `project:` block;
- the example markers in `quota-signals.yml` with vendor wording captured from
  the deployment;
- the GitHub App ids, project id, repository and PEM path in
  `forge/github-app.example.yml` with the App's own values.

## Route join

A fleet domain's `accounts` are logical slots. `host.example.yml`
`provider_accounts.<slot>` binds a slot to this host's working route:
`provider`, `harness`, `native_alias`, `credential` and `serves` (a mapping from
each canonical fleet model to the model id this route actually launches), with
optional `efforts` and, for a cross-provider route or a different launch id, a
`qualification` reference (`<route>@<revision>`). `host.routes.<domain>` orders
the slots this host enables.

A route is eligible only when its `serves` mapping contains the canonical model
and it supports the pinned effort. Without a qualification record,
`serves[model]` must equal the canonical model id; otherwise placement is
refused with `route_model_id_unproven`. A different launch id or a
cross-provider route also needs a hash-bound record in
`orchestration/route-qualifications.yml` covering that route, harness, symbolic
model, canonical id/vendor, route-native launch id and effort. The authored
file has no activation flag: a generated activation record binds its exact
bytes with `source_hash` and carries `activated_at`. A missing, malformed or
unproven reference is refused. The canonical model id, effort and vendor are
unchanged, `route_model_id` is the route's own launch id, and vendor
independence uses the canonical vendor. An incompatible route is skipped and
the configured chain falls through; if no step is compatible, or no route
preserves vendor independence, the placement is refused.
`project.runtime.matrix` is an allowlist of provider/harness pairs, so a host
route pair absent from it is refused before placement. A native alias or working
provider changes only in `host.yml` and never requires a fleet edit.
`tests/fixtures/config/host-alternate.yml` shows a second alias/order mapping and
`tests/fixtures/config/host-cross-provider.yml` shows the same fleet through a
different provider/harness route.

A placement records its resolved slot, native alias, provider, harness,
route-native model id, qualification, canonical model, effort, vendor, policy
hash and host-route hash in an immutable `resolved_route` snapshot. A later host
edit changes only new placements; an active run keeps its snapshot and fails
closed rather than being remapped.

The schema and the migration contract are in
[`docs/CONFIGURATION.md`](../../docs/CONFIGURATION.md#the-selected-config-folder-yaml-only).
