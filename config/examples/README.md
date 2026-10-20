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
    teams/core-team.yml        pinned Core Team roster
  packs/team.yml               project team/profile overlay
  tracker/jira.yml             tracker connection and credential reference
  fleet.yml                    fleet policy, schema_version 2
  host.example.yml             committed host schema example
  host.yml                     this machine only; git-ignored, never committed
  quota-signals.yml            optional vendor refusal wording
  paseo-supervision.yml        optional supervision policy
  .gitignore                   ignores host.yml
```

Paths are classified, not resolved uniformly:

- **Config-file references** (`fleet`, `core_team`, `project.team_pack`,
  `project.fleet`, `project.supervision`, `project.tracker.spec`) resolve from
  the selected folder root. They are relative, must stay inside the folder, and
  must exist.
- **Project resource paths** (`documentation_roots`, `gap_ledger`, `glossary`,
  `decision_records_root`) resolve from the project root the consumer is given,
  never from this folder.
- **Host paths** (`host.paths.*`, `host.binaries.*`, `host.project_cli.*`) are
  absolute or PATH-resolved host state.
- **Conventions** such as `worktree_layout` are patterns, not paths.
- **Credential references** name a secret-store entry; they are never values.

## Bootstrap and activated settings

| Class | Examples | When it is read | Effect of an edit |
| --- | --- | --- | --- |
| Bootstrap | folder selection, `host.yml`, state root, endpoints, ports, credential references | before the daemon starts | requires a restart for new placements; invalid input refuses start; active runs keep their resolved-route snapshot |
| Activated policy | fleet policy, orchestration bundle, Core Team roster, tracker specs, team/profile packs, committee presets | through preview → publish → activate, bound by content hash | no live effect until activated; active runs keep their pins |
| Startup-read | `quota-signals.yml`, supervision policy | at daemon start | validated then; invalid input refuses start |
| Generated projection | a native-format file rendered from YAML | whenever its consumer reads it | never hand-edited; regenerate from its YAML source |
| Test data | fixtures under `tests/fixtures/` | tests only | not deployment configuration |

Authored configuration is YAML only (`.yml` or `.yaml`). Where a runtime or
external tool requires JSON or another native format, that file is a generated
projection with a recorded YAML source and content hash; it is not a second
editable source. Credential values stay in the deployment's secret store; YAML
holds references only.

## Replace before use

- every placeholder account alias in `fleet.yml` and `host.example.yml`;
- every `native_alias`, harness and route order in `host.example.yml` with this
  host's own mapping, and `project_cli.example` with the project's executable;
- every model id in `fleet.yml` with one the runtime catalog accepts;
- the synthetic Core Team catalog identity, content hash and team/committee
  ids;
- the example tracker site and project key in `orchestration/orchestration.yml`
  and `tracker/jira.yml`;
- the organization, repositories, base branch, documentation roots, gap ledger,
  glossary, attention channel, worktree layout, check-run names, capacity, time
  zone, holiday countries and currency in the `project:` block;
- the example markers in `quota-signals.yml` with vendor wording captured from
  the deployment.

## Route join

A fleet domain's `accounts` are logical slots. `host.example.yml`
`provider_accounts.<slot>` binds a slot to this host's working route:
`provider`, `harness`, `native_alias`, `credential` and `serves` (the canonical
fleet models the route is proved to serve), with optional `efforts` and, for a
cross-provider route, a `qualification` naming the reviewed route contract
(`<route>@<revision>`). `host.routes.<domain>` orders the slots
this host enables.

A route is eligible only when it serves the canonical model at the pinned
effort; a cross-provider route without a qualification is refused, never
silently substituted. The canonical model id, effort and vendor are unchanged,
and vendor independence uses the canonical vendor. An incompatible route is
skipped and the configured chain falls through; if no step is compatible, the
placement is refused. A native alias or working provider changes only in
`host.yml` and never requires a fleet edit. `tests/fixtures/config/host-alternate.yml`
shows a second alias/order mapping and
`tests/fixtures/config/host-cross-provider.yml` shows the same fleet through a
different provider/harness route.

A placement records its resolved slot, native alias, provider, harness, model,
effort, vendor, policy hash and host-route hash in an immutable
`resolved_route` snapshot. A later host edit changes only new placements; an
active run keeps its snapshot and fails closed rather than being remapped.

The schema and the migration contract are in
[`docs/CONFIGURATION.md`](../../docs/CONFIGURATION.md#the-selected-config-folder-yaml-only).
