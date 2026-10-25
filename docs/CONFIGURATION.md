# Configuration

> **Stamped to `0971674d` (PR #300, 2026-10-11):** the state-root file inventory, section status labels, the Jira connector and credential contract, fleet, quota-signal, supervision, seat permission posture, seat MCP surface, desk and GitHub App configuration were re-verified against the source at that commit. Merged is not qualified, deployed or enabled for a realm; each section says what its pull request recorded. The 2026-09-05 note below is kept as history.

> **Historical checkout disposition — 2026-09-05 (superseded by the stamp above):** This document accompanies the older `e6da270` branch baseline and preserves its pending/uncommitted work. Its implementation counts and “no engine” statements describe that baseline, not the current released control plane. For released source `082b63ad`, the revision-stamped schema/tool counts and the merged OP-21 supervision and OP-22 convergence are recorded in the release documentation for that source; this historical note does not certify them. Release-documentation corrections are isolated from this checkout’s runtime changes.

Kontor separates invariants from deployment behavior. Rust enforces safety
properties such as one non-terminal session per role slot and uncertainty not
being completion. Durations, prompts, skills, profiles, committees, completion,
budgets and runtime routing are versioned data. One pinned Team Definition JSON
revision owns hierarchy, native prefixes/templates, exact seat labels, roles,
slot capabilities and ordering. See [`NATIVE_NAMING.md`](NATIVE_NAMING.md).

That split is the point, not an implementation detail: the workflow being data is
what lets Kontor run research, architecture, UX, QA and operations work without a
core-code branch per work type, and what lets an operator's own conventions become
system behaviour instead of instructions somebody has to remember.

## The selected config folder (YAML-only)

> **Status:** the folder contract, `host.yml` schema and `project:` block are
> defined by WP-02 (ASMA-8564, workflow-pack plan v0.5). Consumers adopt them in
> WP-05, WP-08 and WP-10; this section is the contract they implement, not a
> claim that every current reader already follows it.

One folder, anywhere on the machine and with any name, holds every authored
setting that varies by project, user or machine. It is the configuration source
for both workflow options: `kontord` and the tools that coordinate through Paseo
(the PR desk, asma-cli and the pack tool). Unmodified Paseo does not read
Kontor YAML; a harness-owned file is a generated projection instead.

### Selection

- Select the folder explicitly with `--config-dir <path>`. When both are set,
  `--config-dir` wins over `KONTOR_CONFIG_DIR`.
- Resolve a relative input once, against the working directory of the setup
  invocation, then persist the absolute selection for later commands and service
  launches. Record and show the resolved absolute path.
- Do not guess the folder from the current directory, a state root, a legacy
  project path or an old state-root file. An absent selection, a missing folder, a missing
  referenced file or invalid YAML is refused by name.
- The same selected YAML source serves Paseo-direct and `kontord`-governed work.
  Setting up the direct option does not require `kontord`.

### Daemon startup

```sh
kontor-daemon --state-root <realm-state-root> --config-dir <config-folder>
```

Serving requires an explicit selection. The daemon reports the canonical
absolute folder and project key; relative references in the entry file resolve
from that folder, including when its name contains spaces. Startup validates
YAML sources and refuses legacy editable sources in the state root with a
migration instruction before claiming the realm. It never chooses a legacy
source as a fallback. Recovery operations on snapshots and exports still work
without authoring configuration. `install-jira-credential` uses the selected
folder and keeps the credential address scoped to the state root and realm.

The first loader slice reads these bootstrap sources:

| Selected-folder source | Contents |
| --- | --- |
| `host.yml` → `runtime.planes` | Existing runtime settings document (`schema_version: 4` or `5`, `runtimes` list), nested under a schema-1 host document |
| `host.yml` → `checkouts` | Optional explicit root/exact-module task Git sources; validated before startup and frozen in durable placement intents before Git effects |
| `tracker/connectors.yml` | Schema-1 connector bindings (`projects` list); separate from `project.tracker.spec`, the tracker specification entry |
| `forge/github-app.yml` | Schema-1 GitHub App settings, with the private key kept outside the repository |
| `orchestration/memory.yml` | Optional memory transport settings |
| `orchestration/succession.yml` | Schema-1 succession runtime selection |
| `paseo-supervision.yml`, `attention.yml`, `quota-signals.yml` | Optional validated watchdog, attention and refusal-classification settings |

An omitted runtime planes document composes an empty registry. This nesting
preserves the existing runtime field contract during migration; logical account
slots and route binding remain separate host settings. The runtime fields below
are the legacy document reference, not a second editable JSON source.

Fleet authoring comes from the entry's `fleet` reference. Published history,
activation records, credentials and the database stay under the state root.
Existing activations and run pins remain authoritative: editing the selected
fleet file or publishing a candidate does not activate it. Embedded callers
that explicitly supply a runtime registry retain their existing composition
seam; the executable uses selected-folder startup.

Project identity is available from the resolved entry at startup. Publication
uses the activated project policy described below; per-realm tracker/team
installation remains a subsequent loader slice.
Clean-start acceptance uses an empty state root and the explicitly selected
YAML folder; startup does not repopulate work from plans or tracker epics.
Generic snapshot and recovery capabilities remain available independently.

Ship the complete config-folder work together in one release after the relaunch
release has been deployed with readback. Loader checkpoints remain draft work
until clean empty-state startup, project isolation and admission are qualified.

### Layout and references

```text
<config folder>/                    explicitly selected; any path, any name
  orchestration/orchestration.yml   entry: schema_version, bundle selectors, project: block
  orchestration/orchestration.schema.yml  the project: block's required-key schema
  orchestration/fleet.yml           fleet policy (schema_version 2)
  orchestration/route-qualifications.yml  hash-bound route qualification records
  orchestration/teams/core-team.yml one pinned Core Team roster
  orchestration/*.yml               workstreams and other project policy
  orchestration/tpm-supervision.yml  autonomous TPM cadence profile
  orchestration/pr-desk.yml         PR desk policy: start budget, active-PR policy, silence windows, round limits
  tracker/*.yml                     tracker connection, field/workflow specs and credential references
  packs/*.yml                       team and profile overlay, committee presets and role prompts
  forge/github-app.example.yml      committed GitHub App placeholder
  attention.yml                     optional attention policy
  attention.env                     placeholder sample for <state-root>/.env; never loaded here
  quota-signals.yml                 optional vendor refusal wording
  paseo-supervision.yml             optional daemon watchdog policy, not a project key
  host.example.yml                  committed host schema example
  host.yml                          this machine only; git-ignored
```

`project.supervision` follows plan §1.1 and names
`orchestration/tpm-supervision.yml`, the autonomous TPM cadence profile. The
Paseo watchdog policy is a different file: configured daemon startup reads
`paseo-supervision.yml` from the selected folder. It is not a key in `project:`.

A path is not one class. The schema distinguishes these, and each class has
its own base and validation:

| Class | Keys | Base | Validation |
| --- | --- | --- | --- |
| Config-file reference | `fleet`, `core_team`, `project.team_pack`, `project.fleet`, `project.supervision`, `project.tracker.spec`, `project.pr_desk.settings` | the selected config folder root | Relative only, contained in the folder, must exist. `..` escapes and absolute paths are refused. |
| Project resource path | `project.documentation_roots`, `project.gap_ledger`, `project.glossary`, `project.decision_authorities.decision_records_root` | a declared `project.resource_roots` entry | The first segment is a root key; the root maps to a repository in `project.forge.repositories`, and the rest resolves inside that checkout. Absolute paths and `..` segments are refused. Never resolved from, or checked for containment inside, the config folder. |
| Desk history path | `project.pr_desk.history_root` | `host.paths.data_root` | Relative only and contained; absolute paths and `..` segments are refused. The learning log is `<resolved root>/learning.log`, resolved by `PR_DESK_DIR`, then `host.paths.pr_desk`, then `<data_root>/<history_root>`. |
| Host path | `host.paths.*`, `host.binaries.*`, `host.project_cli.*` | this machine's filesystem | Absolute or `PATH`-resolved host state. Not a config reference. |
| Convention | `project.worktree_layout`, `project.consultation_layout` | the project's placement patterns | Task pattern uses `<key-slug>`/`<module>`; consultation pattern uses `<key-slug>`/`<node>` under the same managed root. |
| Credential reference | `credential`, `project.tracker.credential_alias` | the deployment's secret store | A non-secret reference such as `keychain:<service>/<account>`; never a value. |

`project.resource_roots` names each checkout the project resources live in, so
one project can span several repositories. ASMA, for example, declares
`modules: Carasent-ASMA/asma-modules` and
`kontor: Carasent-ASMA/asma-rs-kontor`, then writes
`documentation_roots: [modules/_docs, kontor/docs]`. A path whose root is not
declared, whose root is not one of the project's forge repositories, or that is
absolute or contains `..` is refused by name rather than guessed.

Selected-folder startup validates these resource paths without probing for them
inside the config folder. Paths use exact relative segments: empty, `.` and
`..` segments, backslashes, drive/URI prefixes, wildcards and unresolved
placeholders refuse by field and source. A resource directory may retain one
trailing slash. Repository entries must be valid, unique owner/name values;
each resource root must refer to one of them.

The worktree convention must start with a literal managed placement root and
contain exactly one whole `<key-slug>` segment and one whole `<module>`
segment. Unknown or repeated placeholders, absolute paths and traversal refuse
at startup. Validation creates no worktree and changes no existing claim.

An external config folder therefore cannot capture a project's documentation,
gap ledger, glossary or worktree paths: those resolve from the project roots the
consumer supplies. The checked-in neutral example is
[`config/examples/`](../config/examples/README.md); its schema tests are in
`crates/kontor-fleet/tests/config_examples.rs`.

### Bootstrap and activated settings

Moving a file into the folder does not loosen governance. A published policy
keeps the existing preview, publish and activate path, content hashes,
last-valid behaviour and active run pins; an unactivated edit has no live
effect.

| Class | Settings | Read when | Effect of an edit |
| --- | --- | --- | --- |
| Bootstrap | folder selection, `host.yml`, state root, endpoints and ports, credential references | before daemon start | restart required for new placements; invalid input refuses start; active runs keep their frozen `resolved_route` snapshot |
| Activated policy | fleet policy, orchestration bundle, Core Team roster, route qualification records, tracker specs, team/profile packs, committee presets | preview → publish → activate, bound by content hash | no live effect until activated; running work keeps its pins |
| Startup-read | `quota-signals.yml`, `<state-root>/supervision.yml` (Paseo watchdog) | daemon start | validated then; invalid input refuses start |
| Generated projection | a native-format file rendered from YAML | consumer read | never hand-edited; regenerate from its YAML source |
| Test data | fixtures under `tests/fixtures/` | tests only | not deployment configuration |

Authored configuration is YAML only (`.yml` or `.yaml`). Where a runtime or
external tool requires JSON or another native format, that file is a generated
projection with a recorded YAML source and content hash, not a second editable
source. Credential values remain in the deployment's secret store; YAML holds
credential references such as `keychain:<service>/<account>`.

### Project publication authority

`project.branch.types` narrows the shared `kontor-core` branch vocabulary. It
must be nonempty and contain no duplicates. The supported anchored branch
template is `^(type|type)/<KEY>-<number>-<slug>$`, with the declared types in
order; `<KEY>` may instead be the exact `project.key`. The commit template is
`^<KEY>-<number> <summary>$`, with the same literal-key option. Unknown types,
unresolved placeholders, arbitrary regexes and another project's literal key
refuse by field name and selected source. These templates do not replace
canonical branch parsing or confirmed task/epic binding.

Branch rules resolve into the existing publication policy's immutable bundle.
Authoring edits and unactivated publications do not change publication
judgement. Older bundles without branch rules retain the shared grammar;
selected-folder startup requires the authored branch settings. The commit
template is validated and pinned here; publication's head SHA does not attest
to a commit message. Commit-message consumers must observe that message before
claiming it satisfies the template.

The optional pair below configures publication for one existing Kontor Project
in this realm. `project_id` is its explicit internal UUIDv7, distinct from the
public tracker key. Loading this folder never creates a Project.

```yaml
project:
  project_id: "00000000-0000-7000-8000-000000000001" # synthetic example
  forge:
    repositories: [example-org/root, example-org/worker]
    base_branch: main
  publication:
    root_repository: example-org/root
    module_repositories:
      "components/worker": example-org/worker
```

Omitting both `project_id` and `publication` supports direct-only startup with
an empty store and grants no publication authority. If either is present, both
are required, including `root_repository` and `module_repositories`. An empty
module map is valid for a root-only project. Invalid or partial policy is
refused by field name at startup and bundle preview. Repository values must be
exact validated entries in `project.forge.repositories`; membership in that
list alone grants no access. Duplicate or invalid ModuleKeys, wildcards and
unlisted repositories are refused.

Module keys match durable `Task.module` values exactly. They are independent
of `resource_roots`, checkout paths and names. A confirmed task binding admits
the root and only its own mapped module repository. A confirmed epic binding
admits the root and the union for its own stored tasks. An unmapped module adds
nothing. The base is exactly `project.forge.base_branch`, with no fallback.

The existing orchestration preview → publish → activate path resolves this
policy into its immutable manifest, alongside the exact YAML source hashes.
Publication reads that verified activated artifact. Editing authoring files,
publishing without activation or restarting does not replace authority. An
absent policy, invalid active artifact, missing realm Project, mismatched
requested ProjectId, unconfirmed or ambiguous binding or unauthorized
repository fails closed. Fleet placement retains its existing last-valid and
run-pin rules; publication never widens access through a raw YAML fallback.

Any internal `project_id` declared by the selected tracker spec, connector or
GitHub App must agree with this policy. Those integrations constrain authority;
none can supply or override it. A tracker spec without an internal ID is valid.
Policy revision 5 decisions carry `source_bundle_hash` when an activated policy
was read. New accepted receipts require that provenance; older receipts remain
unchanged and do not gain invented hashes. An idempotency key cannot replay an
old acceptance after the governing policy or durable binding changes. The App
poller rejudges unchanged PR heads too and binds its report key to the observed
publication, subject and governing decision.

### `host.yml`

`host.example.yml` is committed beside the neutral example. The real `host.yml`
sits in the selected folder, is git-ignored, and holds this user/machine only.
It is a bootstrap input, never an activated policy.

| Key | Type | Contract |
| --- | --- | --- |
| `schema_version` | integer | `1` |
| `time_zone` | IANA zone | Default zone for human-facing times; a captured vendor message keeps its own zone. |
| `paths` | mapping | `state_root`, `data_root`, `worktrees_root`, `paseo_home`, `logs`; optional `pr_desk`, an absolute desk history root that overrides `project.pr_desk.history_root`; absolute paths on this machine. |
| `binaries` | mapping | Absolute or `PATH`-resolved host commands for `paseo`, `git`, `gh`, `python`, `cargo`. |
| `project_cli` | mapping | `project.cli.command` → this host's executable. A new project supplies its own name; no tool name is assumed. |
| `checkouts` | mapping | Optional task Git sources: required `root` record and `modules` mapping. Each record requires `repository`, `path`, `base_ref`; see below. These are sources, separate from destination layout. |
| `runtime` | mapping | `kontord.endpoint`, `kontord.port`, `paseo.endpoint`, `paseo.port`; loopback addresses only. |
| `provider_accounts` | mapping | Logical fleet slot → `{provider, native_alias, harness, credential, serves, efforts?, qualification?}`. `native_alias` is this host's runtime account id; `credential` is a reference, never a value; `serves` maps each canonical fleet model to the model id this route launches (`route_model_id`); a cross-provider route, or a route with a different launch id, also states `qualification`. |
| `routes` | mapping | Fleet domain → ordered logical slots this host enables. A slot omitted here is unavailable on this host. |
| `keychain` | mapping | `service_prefix` for credential references. |
| `launchd` | mapping | Service labels for the daemon and desk. |

**Task checkout sources.** `host.checkouts.root` identifies the existing source
whose canonical checkout root equals the durable `Project.root_path`.
`host.checkouts.modules` uses exact canonical `Task.module` keys, including all
relative path segments. An exact record selects its source; `{}` is valid.
Without an exact record, the root supplies objects only for a normal tracked
place at the resolved base commit, with no gitlink, symlink or nested repository
boundary. A missing, new or inconclusive place requires an exact record, which
may explicitly choose the root repository. Resource-root aliases, publication
module mappings, repository names and origins never select a source.

Each record's `repository` must be an exact validated
`project.forge.repositories` member. This constrains source identity and grants
no publication permission. Its `path` must be an absolute canonical existing
working-checkout root; discovering a parent repository from a subdirectory is
refused. `base_ref` is exactly `refs/heads/<project.forge.base_branch>` or
`refs/remotes/<configured-remote>/<project.forge.base_branch>`, resolving to a
commit. There is no default-branch search, fetch, clone or implicit remote task
branch fallback. Configuration loading only validates and reads Git.

Task destinations keep the existing contract: `project.worktree_layout`
renders the full durable module and the confirmed tracker key. Root-only tasks
require an explicit validated durable worktree and use the root source; no
module or destination is invented. Deterministic claim correction still needs
a module. An authored ticket link is not confirmation: an undeclared module
task stays unplaced until native materialization reads its confirmed durable
tracker binding. Its first derived claim and source intent commit together.
Existing destinations must be exact checkout roots with the selected
source's Git common directory, a confirmed branch and durable task ownership.
Containment and cross-task/consultation collision guards apply.

Before the first Git effect, a durable placement intent freezes the source
record, canonical common directory, exact base ref, resolved commit, branch and
claim. A pending retry uses that intent. A confirmed placement only verifies
its existing destination; it never recreates a missing running binding.
Host edits require restart for new placements and cannot switch an existing
pin. Guarded claim correction freezes the replacement source with the claim
atomically. Omitting `host.checkouts` permits empty/direct-only startup and
disables managed task source selection without a legacy fallback.

`project.consultation_layout` is an optional relative pattern such as
`consultations/<key-slug>/<node>`. Its root is the durable project root joined
with the literal prefix of the pinned `worktree_layout`: for
`managed/trees/<key-slug>/<module>`, a consultation belongs under
`<project root>/managed/trees/consultations/<epic-key-slug>/<node>`.
Both placeholders must appear exactly once as whole path segments. Absolute,
empty, dot/traversal, unknown and repeated components refuse by field name.
The key is the confirmed canonical epic key; the node is the durable
TopologyNodeId belonging to that epic/project. No task module is substituted.

The existing bundle manifest pins the layout, managed-root pattern, base
branch and shared branch rules together. Each epic uses its frozen bundle;
activation or YAML edits do not relocate existing placements. Omission keeps
empty/direct startup possible and refuses new consultation placement by name.

Before Git or runtime effects, the existing topology node freezes a canonical
intent with its project/epic/key/node, bundle and layout, explicit
`host.checkouts.root` declaration, common directory, resolved base commit,
branch, exact destination and native binding ID. The existing checkout engine
prepares that source and records correlated Git readback before a native
binding can commit. A pending retry uses those exact bytes; a confirmed
placement verifies its existing checkout and refuses a missing destination.
There is no source search, clone/fetch, new registry or request-time YAML read.

Legacy consultations retain their durably bound path and native identity only
when their exact Git root, common directory, branch and ownership still agree.
They are not backfilled or re-rendered. A directory or branch without a durable
intent/binding is refused, including legacy `consultation-<node>` candidates.
Recovery retains the same placement proof; containment alone grants nothing.
The API retains its existing native binding/readback and receipt contracts;
source/layout intents are internal generated evidence, not authored inputs.

**Route join (Q-04).** The fleet owns the canonical policy; the host owns the
working route. They meet on two keys: the logical slot and the canonical model.

1. `orchestration/fleet.yml` owns the canonical model identity and policy:
   model id, vendor, efforts, chains and seat bindings.
   `domains.<domain>.provider` names the model's canonical/native provider
   family; it is not a restriction on a host's working route.
2. `host.yml` `provider_accounts.<slot>` binds the slot to this host's working
   route: `provider`, `harness`, `native_alias`, `credential`, `serves`
   (a mapping from each canonical fleet model to the model id this route
   actually launches), optional `efforts` (a subset of the fleet model's
   efforts), and `qualification` (required when the working provider differs
   from the fleet domain's provider or the route-native launch id differs from
   the canonical model id; the exact reference `<route>@<revision>` into the
   activated qualification records described below).
3. `host.yml` `routes.<domain>` selects and orders the slots this host enables.
   Every route slot must exist in `provider_accounts` and belong to that
   domain. A domain omitted from `routes` is unavailable on this host, so its
   chain descends. The slot name keeps the provider prefix (`claude-`,
   `codex-`, or the bare provider) because the current fleet validator requires
   it; the working route may still be a different provider.
4. Compatibility is checked against the canonical policy, not assumed from the
   slot name. A route is eligible only when its `serves` mapping contains the
   canonical model, maps it to a route-native model id, and supports the pinned
   effort (the route's `efforts`, when stated, contains it). Without a
   qualification record, `serves[model]` must equal `models.<model>.id`;
   otherwise placement is refused with `route_model_id_unproven`. Any other
   launch id, and every cross-provider route, carries a qualification reference
   that matches an activated, hash-bound record covering that exact route,
   harness, symbolic model, canonical id/vendor, route-native launch id and
   effort. `<route>@<revision>` is split exactly once and compared for equality;
   a malformed reference, a missing activation record, a record hash mismatch or
   a record that does not cover the pinned identity, launch id or effort is
   refused. The resolver never substitutes a different provider merely because
   it is available, and never requires a fleet edit to make a host route work.
5. `project.runtime.matrix` is an allowlist of `{harness, provider}` pairs the
   project may launch. A host route whose pair is absent is refused with
   `runtime_pair_not_allowed` before placement; the matrix does not own model
   ids, effort or vendor independence. The neutral example lists
   `paseo/claude`, `paseo/codex`, `paseo/cursor`, `opencode/opencode` and
   `acp-agent/glm-acp-agent`.
6. Fallback skips an incompatible route and continues `routes.<domain>`; if no
   route in the step serves the pinned model and effort, it descends to the
   next chain step. If no step has a compatible route, the placement is refused
   with a typed reason. A fallback never changes the canonical model or effort
   to make a route fit, and never lands on a vendor excluded by
   `rules.independent_of`; if no route preserves real vendor independence, the
   placement is refused with `vendor_independence_unpreserved`.
7. A host edit that changes a native alias, provider, harness or route order
   changes only new placements. Two hosts with different aliases and route
   order resolve the same fleet chain to the same canonical model id, effort
   and vendor; a cross-provider route preserves those canonical values too,
   while `route_model_id` is the route's own launch id.
   `config/examples/host.example.yml`,
   `tests/fixtures/config/host-alternate.yml` (alias/order) and
   `tests/fixtures/config/host-cross-provider.yml` (different provider/harness)
   are the labelled compatibility fixtures. A user's native alias or working
   provider changes only in `host.yml`; it never requires a fleet edit. The
   fleet changes only when the canonical model policy or a logical slot
   changes.

**Qualification records.** A cross-provider route, or a same-provider route
whose launch id differs from the canonical id, is enabled by a record in
`orchestration/route-qualifications.yml`, not by a string in `host.yml`. The
authored file has no activation flag or timestamp. It is published and
activated through the same policy path as the other project policy; a generated
activation record names `source`, binds the exact file bytes with `source_hash`
(SHA-256) and carries `activated_at`. The file is not the fleet policy and not
the host bootstrap file. Each record binds `route`, `harness`, the symbolic
`model`, `canonical_model_id`, `canonical_vendor`, `route_model_id`, `efforts`,
`revision` and `record_hash`. The record hash is SHA-256 over ordered
`key=value` lines joined by newlines, with no trailing newline: `route`,
`harness`, `model`, `canonical_model_id`, `canonical_vendor`,
`route_model_id`, `efforts` in declared order, comma-separated, and `revision`.
Activation time is not part of that hash.
The bundle manifest carries only `qualification: {source, source_hash}` and the
same hash in `sources`. The existing `FleetActivation.activated_at`, linked by
`source_bundle_hash`, supplies activation time; preview and publication keep the
same manifest identity. Exact qualification and orchestration source bytes are
retained by the bundle publisher. The runtime matrix is read from that pinned
orchestration artifact; missing or tampered artifacts refuse without consulting
the current selected folder.
A host reference is exactly `<route>@<revision>`; the resolver splits once,
compares both parts exactly, requires the activation record's `source_hash` to
match the file bytes and `activated_at` to be present, verifies the record hash,
and requires the record to cover the pinned canonical identity, the
route-native launch id and the effort. A changed `serves` mapping or a changed
canonical id/vendor under the same model name no longer matches the record and
is refused. A missing or mismatched activation record, a record hash mismatch,
a malformed reference, or a record whose route/harness/symbolic
model/canonical identity/launch id/effort does not cover the placement is
refused as `route_qualification_unproven`,
`route_qualification_hash_mismatch` or `route_qualification_malformed`. The
record is separate from the fleet, so a new qualified route does not require a
fleet policy edit.

**Successor and migration contract.** The current `schema_version: 2` parser
still validates `domains.accounts` with the provider-prefix rule. That remains
a parser constraint. Governed native placement joins the admitted logical slot
to the activated bundle's runtime matrix and qualifications, then freezes its
host route before a launch or retirement effect. Native readback, retries and
restart recovery use that saved binding. A host route edit affects new
placements without rewriting an active run's snapshot or the fleet policy.

The bundle base also changes. The current reader resolves selectors from
`<config folder>/orchestration/` and requires the manifest names `fleet.yml` and
`teams/core-team.yml`. The successor resolves from the selected folder root:
`fleet: orchestration/fleet.yml` and
`core_team: orchestration/teams/core-team.yml`. Migration maps the old manifest
name `fleet.yml` to `orchestration/fleet.yml` and `teams/core-team.yml` to
`orchestration/teams/core-team.yml`, compares the resolved bytes by content
hash, preserves active pins and refuses a conflicting old and new source. No
manifest reader changes before KR-14.

The Paseo transport announces only its existing protocol number and capabilities,
with no client application version. Readiness still checks the actual server
version against the supported 0.8.0 floor and verifies each required capability;
this cleanup does not qualify a new server release or advance KR-14.

Schema 143 renames the activation authority to `epic_activations` and its command
kind to `activate_epic` through the existing ordered migration transaction. It
retains receipt IDs, canonical intent bytes/digests, frozen pins and activation
rows; only the typed command kind and table name change. A retry compares all
operation arguments and reuses the frozen receipt intent. Older binaries must
refuse the newer schema; rollback requires a compatible binary, never a forced
schema-version downgrade. The historical migration CHECK lists remain frozen.

For ASMA, the selected folder is `asma-modules/config/`. Its successor
references are `fleet: orchestration/fleet.yml`,
`supervision: orchestration/tpm-supervision.yml` (the TPM cadence profile),
`tracker.spec: tracker/jira.yml`, `pr_desk.settings: orchestration/pr-desk.yml`
and `team_pack: packs/team.yml`; its project
resources use `resource_roots` `modules`/`kontor` against the two repositories.
The Paseo watchdog policy is not a project key: it is
`<state-root>/supervision.yml`. The fixtures above are labelled for that
migration and are test data, not deployment files.

The vendor-independence rule uses the canonical `models.<name>.vendor`, never
the working provider or harness. A `glm-acp-agent` route that serves a canonical
model with vendor `example` is still `example` for the reviewer-vendor rule, so
a cross-provider substitution cannot silently defeat independence. Current
availability, funding and quota remain observed runtime state, not copied YAML
facts.

**Active-run resolved-route snapshot (PIN-001).** A placement freezes one
immutable `resolved_route` before the run starts:

```yaml
resolved_route:
  policy_hash: <fleet policy content hash>
  host_route_hash: <hash of host.yml and the resolved slot order>
  slot: opencode
  route_provider: glm-acp-agent
  route_harness: acp-agent
  native_alias: desktop-glm-acp-1
  qualification: glm-acp-agent@1
  route_model_id: glm-5.3
  canonical_model: example/model
  canonical_effort: max
  canonical_vendor: example
  resolved_at: <timestamp>
```

The snapshot records the working route (`route_provider`, `route_harness`,
`native_alias`, `qualification`, `route_model_id`) and the canonical policy
(`canonical_model`, `canonical_effort`, `canonical_vendor`). `route_model_id`
is the id the working route actually launches; it may differ from
`canonical_model`. It is authoritative for that run.
The private carrier has `schema_version: 1` and is stored in existing launch
route columns as `{schema_version: 1, resolved_route: {...}}`. Canonical seat
bindings and public provenance remain canonical. Historical bare model rungs
remain readable without rewriting them or inventing host/policy pins. An
invalid envelope cannot be interpreted as a legacy rung. Under a selected
config, historical bare rungs do not authorize new launches or permission
effects. A policy-bound freeze requires its exact source bundle pin; an unbound
placement may freeze against one activated bundle before effects.

Hosted launches and governed claims retain it in their generation's launch
intent before changing native state. Claims verify the observed native route
against that snapshot and keep the canonical route in the seat row. Consultation
initial launches use `consultation_seats.model_rung`; recovery and reroute use
their existing private route columns. Delivery launches retain the binding with
the existing fleet decision; replacement command intents and quota succession
intents freeze it before retiring a predecessor. Replay and compare-and-swap
compare the complete stored binding, including private route identity.
Consultation contexts, receipts, seat policy and public provenance project only
canonical routes. The working provider determines the native permission mode;
any required permission grant remains bound to the exact logical slot.

`host_route_hash` is SHA-256 of compact UTF-8 JSON with keys in exactly this
order: `host_sha256`, `domain`, `slots`. `host_sha256` hashes the exact `host.yml`
bytes; `slots` is the domain's exact ordered slot list. Native aliases remain
private launch data and are absent from public route projections and errors.

Editing `host.yml` or the fleet policy never rewrites an active run's snapshot:
it is read from the run's launch intent and receipt, not re-resolved from the
current files. A daemon restart
keeps the same snapshot for work that resumes on the same run. A new placement
— a fresh run, a replacement, or an explicit reroute — resolves anew and
records a new snapshot under the existing recovery and reroute rules; an
unactivated policy edit still has no effect. If a slot or native
alias named by an active snapshot disappears, the run is not silently remapped
to another account; it fails closed with a typed reason and follows the normal
recovery path. Implementation belongs to WP-08; this contract is what its
consumers must record and preserve.

### The `project:` block in `orchestration.yml`

The block is additive to the existing bundle selector (`schema_version`,
`fleet`, `core_team`). Until WP-08 extends the bundle reader, the selector keys
are the only ones read; this document and the schema test define the block so
the readers cannot drift apart. The machine-readable key list is
[`config/examples/orchestration/orchestration.schema.yml`](../config/examples/orchestration/orchestration.schema.yml);
the neutral example is
[`config/examples/orchestration/orchestration.yml`](../config/examples/orchestration/orchestration.yml)
and ASMA's concrete values are the test case in
[`tests/fixtures/config/asma-orchestration.yml`](../tests/fixtures/config/asma-orchestration.yml).

| Key | Type | Meaning |
| --- | --- | --- |
| `key` | string | Confirmed tracker key; the public identifier. |
| `name` | string | Project display name. |
| `owner` | string | Role, not a person; the escalation owner. |
| `decision_authorities` | mapping | `platform`, `architecture`, `decision_records_root`. |
| `tracker` | mapping | `kind`, `site`, `project_key`, `key_pattern`, `credential_alias`, `spec`. |
| `branch` | mapping | `types`, `pattern`, `commit_pattern`; the grammar, not a copy of one branch. |
| `forge` | mapping | `kind`, `organization`, `repositories`, `base_branch`. |
| `runtime` | mapping | `matrix` of `{name, harness, provider}` launch combinations. |
| `cli` | mapping | `command` and `verbs` for text and checks that must not hard-code one tool; `host.project_cli.<command>` binds the executable. |
| `team_pack` | path | Folder-relative YAML manifest selecting profile/team, operational-domain and committee candidates; see the selected-pack contract below. |
| `fleet` | path | Folder-relative fleet policy; must agree with the bundle selector. |
| `supervision` | path | Folder-relative `autonomous_tpm_v1` cadence profile. Not the Paseo watchdog policy, which is `<state-root>/supervision.yml`. |
| `pr_desk` | mapping | `command` (the desk invocation line), `settings` (folder-relative `orchestration/pr-desk.yml`), `history_root` (relative to `host.paths.data_root`); see the PR desk contract below. |
| `merge_authority` | mapping | `role`, `policy`. |
| `documentation_roots` | list | Project documentation roots. |
| `gap_ledger` | path | Project gap ledger. |
| `attention_channel` | string | Where typed attention is raised. |
| `glossary` | path | Project glossary. |
| `worktree_layout` | string | Checkout convention, with `<key-slug>` and `<module>` placeholders. |
| `consultation_layout` | optional string | Relative consultation pattern with exactly one whole `<key-slug>` and `<node>` segment, under the managed-root prefix of `worktree_layout`. |
| `check_runs` | list | Required check-run names. |
| `capacity` | mapping | `max_delivery_seats`, `max_parallel_tasks`, `start_admission`; see the capacity rules below. |
| `time_zone` | IANA zone | Project default. |
| `holiday_countries` | list | Required, nonempty uppercase two-letter ISO country codes available to holiday import. Imports refuse countries outside this selection; Nager has no compiled country default. The provider validates its own support. |
| `currency` | string | ISO 4217 code for budgets and costs. |

`max_parallel_tasks` and `start_admission` are the Paseo-direct placement
capacity ceiling and initial admission: ASMA 7 and 4. `start_admission` must be
less than or equal to `max_parallel_tasks`. `max_delivery_seats` is an
owner-set project value scoped per team run (ASMA 2); the placement rule does
not supply it, it is not a global team-run ceiling, and kontord has no such
field.

### Selected team, profile, domain and committee authoring

`project.team_pack` names a schema-version-1 manifest. Its required keys are
`profile_pack`, `operational_domain` and `committee_presets`, each a contained,
existing YAML reference relative to the selected folder root. The owning typed
parsers validate the complete documents and exact revision references; logical
roles used by teams and committees must have operational delivery bindings.
Manifest additions are refused rather than ignored. The four raw source hashes
are captured at startup, without publishing, activating or creating a Project.
Pack sources are regular files owned by the selected folder's owner, without
group/world write permission and at most 1 MiB each. Parsing and hashing use
the same captured UTF-8 bytes. The domain source must validate its delivery
kinds, unique logical-role bindings, current catalog roles, sparse prompts and
exact Team Definition/topology references.

Only selected and explicitly registered profiles/teams are advertised on the
selected-folder surface. Omission in an empty/direct-only daemon setup has no
compiled profile or preset fallback. Selection supplies candidates for existing
preview/apply paths, not permission to rewrite an installed revision. Selected
teams use registered-input authority, so the compiled-seed reconciliation path
cannot overwrite them. Committee candidates require explicit preview/apply;
catalog reads and restart retain installed bytes and do not publish edits.
The neutral example has generic roles, no accounts, no clinical scenarios and
no project prompt. Worker model routes come from activated fleet bindings.

New Core Team resolution uses the selected operational domain and its exact
catalog snapshot. The existing delivery keys `architecture_closer_code`,
`process_closer_code` and `control_role_code` derive read-only resolved seat
`purposes`: `architecture_closer`, `process_closer`, `control`, in that fixed
order without duplicates. They are not authored seat fields or caller privileges.
Exact `RoleCode` equality assigns them; labels, ordering and `ad_hoc_allowed`
grant nothing. Each purpose has one required owner, architecture and process
remain separate, and process plus control can occupy the same seat.
Missing, ambiguous or non-current role selectors refuse by their delivery field
name; lead and program role codes have no compiled fallback.

Published and epic-pinned rosters retain these markers with their exact
catalog/version/role/slot bindings. Later delivery edits, restart or publication
of another Core Team cannot reinterpret a frozen roster. Legacy rows without
`purposes` remain readable and serialize without the empty field; they grant no
purpose authority. A dependent operation refuses `role_purpose_unpinned` until
an independently authorized operator explicitly previews/applies a tagged Core
Team revision and explicitly upgrades the epic roster. Startup and reads never
backfill markers. Selected-folder epic bootstrap requires a published Core Team.

Project and epic Team Definition selection records have an optional
`operational_domain_bundle_hash`, pointing to the existing canonical bundle
manifest rather than changing definition/topology identity. Schema 139 adds the
nullable columns to the existing tables without filling historical rows. A
project selection compares the expected domain hash alongside the expected
definition; an epic pin remains immutable when the project default changes.
Absent fields deserialize as no pin and stay omitted on serialization.

The existing definition-retitle migration intent does not bind domain context.
It refuses `operational_domain_upgrade_unsupported` for a domain-pinned epic;
an in-flight retitle also prevents first attaching a domain pin. Historical
retitle intents and reads retain their existing behavior. Generic exports use
generation 18 to preserve these fields; older generation/claimed database
combinations cannot discard or invent them. Historical unpinned record hashes
remain unchanged. This does not create or restore an installation backup.

Explicit project Team Definition selection requires a verified activated domain
bundle. The published definition and topology must be exact members of that
domain. A stored catalog under the same identity must agree; an existing Core
Team must retain its frozen purposes under that same catalog and delivery.
Preview binds the current and proposed domain bundle hashes and exact roster,
including when the definition is unchanged. Prompt-only or delivery-only edits
therefore require publication, activation and a new selection preview. Apply
compares the project, roster and selection again and commits the selection and
its exact command result atomically. Schema 140 extends the existing exact-intent
local-result ledger to this command; it adds no table or pointer. Replay reads
that original result even if
the project default or activation later changes; it never re-selects current
YAML. Historical receipts without a frozen result refuse
`operational_domain_selection_result_unavailable` instead of inventing one.

Epic preview and apply require an explicit project domain selection and published
Core Team with frozen purposes. They verify the immutable bundle, exact published
Team Definition/topology/catalog and roster agreement. New creation copies the
domain hash, definition, topology and exact roster in the same transaction as the
graph and its creation intent/result/receipt. Schema 141 admits epic creation to
the existing exact-intent local-result ledger; no new table, pointer or backfill
is introduced. A changed selection or roster between resolution and commit
refuses and rolls back all of these writes. The receipt remains
`intent_persisted` while kickoff holds, origin fencing or governance are
unconfirmed. The existing success hook confirms it only after the application
returns successfully; a retry resumes the original frozen creation context.

Preview and apply return `operational_domain_bundle_hash`. A client can send it
as `expected_operational_domain_bundle_hash` to bind apply to that preview;
a different target refuses `operational_domain_epic_target_moved`. Omission
selects the explicitly configured project domain at apply time. The creation
intent and graph digest bind the resolved domain even when the client omits the
precondition. An existing epic uses its frozen pin, independently of later
activation, selection, YAML edits or restart. Receipt replay verifies its original
result and pin; it does not resolve today's selection. Historical receipt readback
remains available without granting new domain effects. An unpinned existing epic
cannot acquire domain authority by reapplying it: `operational_domain_unpinned`.

Epic runtime placement, control-role checks, consultation kind selection and
admission resolve the epic's persisted domain bundle. Every resolution verifies
the immutable artifact hash and its published Team Definition, topology, catalog
and frozen roster. Current YAML, project defaults and later activation cannot
reinterpret that epic. Missing pins refuse `operational_domain_unpinned`; missing
or changed artifact bytes refuse before domain-dependent effects. Recorded
roster, occupancy and persona history remains readable without granting effects.

A new epic seat launch records its persona (including an absent prompt) atomically
with its existing launch intent. Recovery reuses that exact stored persona;
it never fills a missing historical persona from current config. Roster upgrade
validates the proposed roster against the epic's domain before changing the pin,
and its preview hash includes the domain identity when present.

Project Quick sessions, project Desk Adams and PR-review seat persona selection
retain their existing behavior in this checkpoint. Quick promotion has no eligible
frozen domain snapshot: promotion can retain its intent but epic materialization
refuses `operational_domain_unpinned`, with no native launch or handoff. A retry
cannot acquire authority from today's project selection. Qualifying those project
consumers needs a separate snapshot/recovery contract. Native consultation
placement remains a later WP-08 checkpoint; selecting its pinned kind does not
change its placement contract.

### The PR desk contract

`project.pr_desk` is the owning parameter block for the project's PR desk:

- `command` is the command line that invokes the desk in the project checkout.
  ASMA's is `python3 _tools/ai-orchestration/pr_desk.py`; a neutral project
  supplies its own executable or script.
- `settings` is a config-file reference to the pinned name
  `orchestration/pr-desk.yml`. It resolves from the selected config folder,
  stays inside it, and must exist.
- `history_root` is the desk history root relative to `host.paths.data_root`.

The learning log has exactly one resolved root, in this precedence order:

1. `PR_DESK_DIR`, when set. The desk keeps this environment variable as the
   explicit top-precedence host override.
2. `host.paths.pr_desk`, when present, an absolute history root in host
   bootstrap data.
3. `<host.paths.data_root>/<project.pr_desk.history_root>`, the project
   default.

The learning log is `<resolved root>/learning.log`. There is no second
candidate. ASMA uses the host override: its `host.yml` sets
`host.paths.pr_desk` to the current desk folder, so the learning log stays
where it is and the live path stays in host data, not this repository.

`orchestration/pr-desk.yml` carries `schema_version` and six desk policy keys:
`poll_start_budget`, `active_pr_policy`, `ready_silence_days`,
`draft_silence_days`, `fix_round_limit` and `revise_round_limit`. The budget,
the two silence windows and both round limits are positive integers.
`active_pr_policy` is `unlimited` or `one_active`. Project-owned keys beyond
those seven are allowed beside them, including ASMA's `desk`,
`skip_repositories` and `durable_seats`. The neutral example uses sample
values; ASMA's values are in the schema test fixture.

ASMA's values, exercised by the schema test:

| Key | ASMA value |
| --- | --- |
| `key`, `tracker.project_key` | `ASMA` |
| `tracker.kind` | `jira` |
| `tracker.key_pattern` | `^ASMA-[0-9]+$` |
| `forge.kind` | `github` |
| `forge.organization` | `Carasent-ASMA` |
| `forge.repositories` | `Carasent-ASMA/asma-modules`, `Carasent-ASMA/asma-rs-kontor` |
| `forge.base_branch` | `master` |
| `cli.command` | `asma` |
| `project.supervision` | `orchestration/tpm-supervision.yml` (`autonomous_tpm_v1` TPM cadence profile; the Paseo watchdog is `<state-root>/supervision.yml`) |
| `project.pr_desk.command` | `python3 _tools/ai-orchestration/pr_desk.py` |
| `project.pr_desk.settings`, `project.pr_desk.history_root` | `orchestration/pr-desk.yml`, `pr-review` |
| `host.paths.pr_desk` | ASMA's `host.yml` sets it to the current desk folder (host data, not committed), so the learning log stays there |
| `orchestration/pr-desk.yml` policy | `poll_start_budget: 3`, `active_pr_policy: unlimited`, `ready_silence_days: 3`, `draft_silence_days: 15`, `fix_round_limit: 2`, `revise_round_limit: 2` |
| `documentation_roots` | `modules/_docs`, `kontor/docs` |
| `check_runs` | `asma/publication-branch-title`, `asma/publication-identity` |
| `capacity` | `max_delivery_seats: 2` (owner-set, per team run), `max_parallel_tasks: 7` (Paseo-direct placement ceiling), `start_admission: 4` (placement start) |
| `time_zone`, `holiday_countries`, `currency` | `Europe/Oslo`, `[NO]`, `NOK` |

## Configuration authoring inventory (WP-02 refresh)

This refresh replaces the 2026-10-17 snapshot for WP-02 at the current
revision. It covers every human-edited JSON source and its readers, the YAML
replacement, the migration and any generated native-format projection. A
source that is generated, test data, evidence or a tool-native manifest is
recorded as such instead of being migrated.

| Source | Class and reader | Disposition | Migration and projection | Owner |
| --- | --- | --- | --- | --- |
| `config/examples/orchestration/fleet.yml` | neutral example; tests | Keep neutral schema-version-2 example. The runtime/parser tests use `tests/fixtures/config/fleet.yml`. | None; no real account is named. | WP-02 (fixed) |
| `config/examples/quota-signals.yml` | neutral example; tests | Keep neutral example. The parser tests use `tests/fixtures/config/quota-signals.yml`. | None; example markers match no live vendor copy. | WP-02 (fixed) |
| `docs/CONFIGURATION.md` GitHub App example | documentation | Placeholders only. | None. | WP-02 (fixed) |
| `<state-root>/jira.json` | human-edited; daemon Jira connector | YAML `tracker/jira.yml`. | Read the old file once, back it up, compare resolved values, then refuse both old and new sources together; project the native JSON only if a consumer still requires it. | WP-08, WP-10 |
| `<state-root>/config/github-app.json` | human-edited; daemon publication gateway | YAML `forge/github-app.yml` plus a credential/PEM reference. | Same backup/equivalence migration; keep the PEM outside the repository. Native JSON is a generated projection with provenance. | WP-08 |
| `<state-root>/memory-cognee.json` | human-edited; daemon experience transport | YAML `orchestration/memory.yml`. | Same migration; disabled stays the default. | WP-08 |
| `<state-root>/runtimes.json` | human-edited; daemon runtime composition | YAML `host.yml` (`runtime`, `provider_accounts`, `routes`) plus project `runtime.matrix`. | Migrate per host; no credential value moves into YAML. | WP-08 |
| `<state-root>/succession-summarizer.json` | human-edited; daemon succession | YAML `orchestration/succession.yml`. | Same migration; absent keeps the typed degraded handoff. | WP-08 |
| `<state-root>/fleet-activation.json`, `credentials.json`, `endpoint.json` | generated or secret state | Keep in the state root. | Never an authoring source; the activation record remains a generated pointer. | WP-08 |
| `config/orchestration/workstreams.json` (ASMA) | human-edited; asma-cli, PR desk and mirrored skills | YAML `orchestration/workstreams.yml`. | Move, update every reader and the skills, then remove the JSON path. | WP-10 |
| `config/orchestration/pr-desk.yml` (ASMA) | human-edited desk decision file; `pr_desk.py` reads `desk`, `skip_repositories` and `durable_seats` | Keep it as the project's `project.pr_desk.settings` file. Add the six policy keys (`poll_start_budget`, `active_pr_policy`, `ready_silence_days`, `draft_silence_days`, `fix_round_limit`, `revise_round_limit`) beside `schema_version`; extra project-owned keys are allowed beside the required seven. | Its `desk.desk_folder` key retires in favour of `project.pr_desk.history_root` plus `host.paths.pr_desk`. ASMA's host sets `host.paths.pr_desk` to the current desk folder so the learning log stays in place. `PR_DESK_DIR` stays the explicit top-precedence host override. The live desk path stays in `host.yml`, never in this public tree. | WP-10 |
| `crates/kontor-teams/fixtures/mvp-team-pack.json` | human-edited seed compiled by `kontor-teams/src/spec.rs` | YAML team pack in the selected folder; neutral default in the pack. | Back up, convert, compare resolved teams and slots, update the reader, remove the bundled editable copy. A native JSON projection is generated only if a runtime still reads it. | WP-08 |
| `crates/kontor-profiles/fixtures/mvp-profile-pack.json`, `operational-domain.json`, `consultation-presets.json` | human-edited seeds compiled by `kontor-profiles/src/seeds.rs` | YAML profile/domain/preset packs in the selected folder. | Same conversion and equivalence check; ASMA's overlay moves to its folder, the neutral default stays neutral. | WP-08 |
| `crates/kontor-jira/tests/fixtures/*.json` | Test-only historical tracker documents; no deployment compiles them. | ASMA's six authored specs move to its selected `tracker/*.yml`; neutral folders declare their own specs or none. | Selected catalogs offer candidates; explicit Admin installation atomically publishes field/workflow hashes per realm/project. Installed bytes keep their immutable pins. | WP-08 |
| `crates/kontor-mcp/seats/*.json` | human-edited MCP seat configurations; external clients, `crates/kontor-mcp/tests/seats.rs` and `seats/README.md` | YAML seat profiles in `packs/`, rendered to the native seat files. | Generate the three native JSON files from YAML with source provenance; do not add a second authority model. | WP-08, WP-10 |
| `plugin-config/kontor/floor.json` | operator-owned server file; Paseo plugin server start (KN-04 / ASMA-8551) | **Proposal:** YAML source `orchestration/plugin-floor.yml`; generated projection at `<PASEO_HOME>/plugin-config/kontor/floor.json`. | See the KN-04 proposal below. The decision's value table, read-once/reload-only behaviour and doubt-leaves-floor-on rule are unchanged. | WP-05, WP-08, KN-04 successor |
| `opencode.json`, `.github/mcp.json` | harness-native files (ASMA) | Generated projections from YAML. | Q-03: no handwritten JSON exception; render deterministically and record the YAML source. | WP-11 |
| `.github/agent-policy-inventory.json` | generated inventory read by agent-policy checks | Keep generated. | Render deterministically from the source metadata; never hand-edit. | WP-11 |
| `.cursor/agent-policy-inventory.json` | generated inventory read by Cursor adapter checks | Keep generated. | Same generated-projection rule as the GitHub inventory. | WP-11 |
| `.cursor/mcp.json` | symlink to `.github/mcp.json` in ASMA, not a separate projection | Keep the symlink; the target is the generated projection. | Regenerate the target from the YAML MCP source with provenance; no handwritten JSON exception. | WP-11 |
| `.claude/settings.json` | tool-native Claude harness settings | Keep; not Kontor authoring configuration. | No Kontor migration; WP-11 reviews any project value it carries. | Not a WP-02 migration |
| `.agentsroom/project.json` | tool-native AgentsRoom project settings | Keep; not Kontor authoring configuration. | No Kontor migration; WP-11 reviews any project value it carries. | Not a WP-02 migration |
| `apps/paseo-plugin/paseo-plugin.json`, `package.json`, `tsconfig.json`, `apps/desktop/src-tauri/tauri.conf.json`, `capabilities/default.json` | tool-native manifests and build metadata | Keep; not Kontor authoring configuration. | None. | Not a WP-02 migration |
| `crates/kontor-api/contract/openapi.json`, `apps/console/src/api/schema.d.ts`, `crates/kontor-context/schemas/*.json` | generated contracts and schemas | Keep generated. | Regenerate from source; never hand-edit. | Not a WP-02 migration |
| `apps/paseo-plugin/test/fixtures/*`, `apps/paseo-plugin/test/teams-source-*.test.ts*` | synthetic test data; not Kontor authoring configuration | Keep as tests; ASMA-8539 replaced the live aliases with synthetic values and a synthetic home. | None for WP-02; WP-12 owns any remaining synthetic-value pass. | WP-12 |
| `scripts/*.json` (`durable-desk-mutants.json`, `test-timings.json`), test fixtures and evidence JSON | machine data, test data and evidence | Keep. | None. | Not a WP-02 migration |

### KN-04 projection proposal (decision unchanged)

KN-04 (ASMA-8551) selected an operator-owned server file at
`<PASEO_HOME>/plugin-config/kontor/floor.json`, read once at plugin server start
and re-read only by a supported install/reload. The file has one key, `floor`,
with exact value `on` or `off`; formatting whitespace is permitted inside the
value, every other state leaves the floor enabled with a structured diagnostic,
and an absent file is enabled. The decision is recorded in the navigator plan
(Revision 1.4) and is not changed here.

**Proposal for the YAML-only rule.** Author the value once in the selected
config folder as `orchestration/plugin-floor.yml`:

```yaml
schema_version: 1
floor: 'on'   # 'on' or 'off'; invalid or absent means floor-on
```

Generate `<PASEO_HOME>/plugin-config/kontor/floor.json` from that source with a
recorded source path and content hash, and keep the plugin reading the JSON file
exactly as KN-04 specifies. This preserves the decision's read-once/reload-only
boundary, the `on`/`off` value table, the doubt-means-on rule and the
plugin-disable rollback. It requires a successor decision or an explicit
KN-04 amendment plus the WP-05 generator; WP-02 records the proposal and does
not change the plugin or its decision.

### WP-02 inventory rows fixed

| Original row | Disposition |
| --- | --- |
| `config/examples/orchestration/fleet.yml`, `quota-signals.yml` carried owner aliases, dated switch-offs and real account wording | Neutral examples hold placeholder accounts and fake markers; the test fixtures' narrative comments are stripped. Tests use `tests/fixtures/config/` and pass without the owner's fleet. |
| `docs/CONFIGURATION.md` GitHub App example carried a real project UUID, a home path and Carasent repositories | The example uses placeholders only. |
| ASMA `config/orchestration/fleet.yml` carried the owner's four provider aliases | `host.yml` owns the host-local account mapping; the fleet policy names domains and account slots. The schema is defined here; WP-08/WP-10 migrate the ASMA reader and file. |

## Where configuration lives

| Location | Holds | Schema; when absent |
| --- | --- | --- |
| Selected config folder (§1.1) | Authored settings that vary by project, user or machine: orchestration, tracker, packs, fleet, quota signals and `host.yml` | YAML only; selected explicitly with `--config-dir` or `KONTOR_CONFIG_DIR`; a missing or invalid folder is refused by name |
| `<state-root>/kontor.db` | Every versioned specification published through `/v1`: Team Definitions, topology specs, role catalogs, work profiles, team templates, advisor profiles, committee templates, completion profiles, Core Team revisions, connector field/workflow specs; also Team Definition defaults, epic pins and migration evidence, desk, durable-seat, desk-Adam and PR-review records, and the durable capacity ceilings, which are read once at startup | Store migrations through v136; created and migrated at first start |
| `<state-root>/catalog.db` | The observational work catalog's own database: logical plans, occurrences, content revisions, aliases, explicit/declared associations, sweep pages, generations, diagnostics and conflicts; the separate read-only Jira observation generation, pages, aliases, coverage and tombstones; and the native Paseo projects, workspaces, sessions, census pages and native generation. Separate from `kontor.db`; it never gates scheduling | Catalog schema version 3 (v1 plan tables, `0002_jira_evidence.sql`, `0003_native_census.sql`); created and migrated when `kontord` starts. A failure opens no catalog and leaves governed service untouched |
| `<state-root>/catalog.json` | Operator-owned effective catalog scope: repositories/worktrees, permitted `.github/plans`, imported operator sources with their IDs and pins, registry path, cadence, scan budget, and an optional read-only `jira` section (project, scope JQL, declared keys, endpoint/credential reference, cadence and bounds). Migrated once from the effective plugin configuration because `launchd` does not inherit its environment | `schemaVersion: 1`, camelCase; absent: the catalog migrates the plugin configuration when one exists, otherwise zero repositories is a valid catalog; absent `jira`: no Jira collector |
| `<state-root>/catalog-snapshot.json` | Atomic fallback snapshot of the last finished catalog generation, read only when the database read fails | Contract `schema_version: 1`; generated after each finished sweep. An incompatible snapshot is refused rather than misread |
| `<state-root>/catalog-native-snapshot.json` | Atomic fallback snapshot of the last complete native Paseo census, read only when the database read fails. Its `written_at` is the census completion instant, never the read instant | Native census `schema_version: 1`; generated after each complete census. An incompatible snapshot is refused rather than misread |
| `<paseo>/plugin-config/kontor/work-catalog-snapshot.json` | The plugin's own versioned last-known projection of the shared catalog, read only when the observer MCP path is unavailable or incompatible. The plugin never opens `catalog.db`. `savedAt` is when the plugin checked; every presented freshness instant is a source `asOf` | `schema: kontor/plugin-work-catalog@1`, `version: 1`; written atomically after a successful observer read. An incompatible snapshot is ignored rather than misread |
| `<state-root>/kontor.lock` | The exclusive state-root lock a serving daemon, and every stopped-realm operator command, holds | Generated |
| `<state-root>/credentials.json` | The realm's three tier secrets, `0600`, read by the CLI and every MCP server | `schema_version: 1`; generated at first start |
| `<state-root>/endpoint.json` | The bound loopback `base_url`, rewritten after every bind | `schema_version: 1`; generated. Clients read it when present and otherwise use `http://127.0.0.1:7717` |
| `<state-root>/runtimes.json` | Runtime family, plane endpoint, per-account provider aliases, the plane's default seat posture, and per-plane outage controls (`unavailable_providers`, catalogued `provider_fallbacks`) | Generation `5`; generation `4` is read as a `5` that declares no posture, which resolves to `ask`; generation `3` is refused rather than upgraded, because it can compose the right sessions under misleading names. Absent: no runtimes, and session routes answer unconfigured |
| `<state-root>/succession-summarizer.json` | Optional governed succession summarizer selection: `{"schema_version": 1, "runtime_family": "<family>"}`. Its absence keeps the typed degraded handoff and dispatches no summary. A present document refuses the daemon start when it is malformed, names a family the composed fleet does not hold, or names a runtime that does not declare the `summarize` capability. No shipped runtime supports summaries yet — Paseo, Codex and AO do not declare it — so only a runtime implementing the governed summary surface can be selected; until one ships, summaries stay degraded and ASMA-8340 stays open | `schema_version: 1`; absent: typed degraded handoff. Present but malformed, an unknown family or a runtime without the `summarize` capability refuses the start |
| `<state-root>/supervision.yml` | Optional seat supervision policy. Schema v1 is validation/classification only; schema v2 can explicitly enable resident bounded succession (see below) | `schema_version` 1 or 2; absent: no supervisor |
| `<state-root>/quota-signals.yml` | Vendor exhaustion wording, applied to a seat's own refusal text (see below) | `schema_version: 1`; absent: refusal classification is inert. Present but invalid refuses the start |
| `<state-root>/fleet.yml` | Live model routing — domains, accounts, models, chains and seat bindings, read at every placement (see below) | `schema_version: 1`; absent: template routing. Not read while `fleet-activation.json` exists |
| `<state-root>/fleet-activation.json` | Generated: which published fleet policy placement reads, by content hash — and, for an aligned (schema_version 2) activation, which orchestration bundle and Core Team roster. Written only by activation; while it exists, `fleet.yml` is not read (see below) | `schema_version` 1 or 2; absent: unmigrated `fleet.yml` behaviour |
| `<state-root>/fleet-status.json`, `fleet-history/`, `fleet-decisions/` | Generated: the last `fleet.yml` load result, accepted and published policy copies by hash, and per-placement decision and launch-provenance logs (see below) | Generated |
| `<state-root>/core-team-history/`, `<state-root>/orchestration-history/` | Generated, immutable: canonical Core Team revisions and orchestration bundle manifests, each named by its content hash; `orchestration-history/domains/` retains exact domain YAML bytes (see below) | Generated by bundle publication |
| `<state-root>/jira.json` | Jira connectors keyed by Kontor project id: endpoint, project key, credential alias, the confirmed epic keys automatic reconciliation may touch, and extra create fields (see below) | `schema_version: 1`; absent: Jira is not configured. Present but invalid refuses the start |
| OS keychain service `kontor-jira:<realm-id>:<sha256 of the canonical state root>` | The Jira `email`/`api_token` for each alias `jira.json` names, written only by `kontor-daemon install-jira-credential` (see below) | Absent: Jira calls for that alias fail as an `unavailable` credential refusal |
| `<state-root>/config/github-app.json` | Optional GitHub App gateway for publication identity; its PEM key lives outside every repository (see below) | `schema_version: 1`; absent: no forge check is posted. Present but invalid refuses the start |
| `<state-root>/memory-cognee.json` | Optional bounded experience-retrieval configuration; disabled by default (see below) | No version field, strict JSON, at most 8 KiB; absent: lexical recall only. Present but invalid refuses the start |
| `<state-root>/provider-homes/` | One credential home per provider account — `CODEX_HOME` for Codex, `CLAUDE_CONFIG_DIR` for Claude. At startup the daemon reconciles each owned Codex home's `config.toml`: `codebase-memory-mcp` is made non-required and the `kontor` server's `env_vars` names `KONTOR_AUTH`; no secret value is written | Absent: no approved account homes |
| `<state-root>/runtime-roots/` | Generated: daemon-owned working directories, including each desk's `<project id>/desk-<desk node id>` and each PR review room's `…-room-<room node id>` | Generated |
| `<state-root>/hosted-directory-publications/` | Generated, private: the recovery journal for seat MCP composition into a desk's non-Git directory (see Seat MCP surface) | Generated |
| `<state-root>/backups/` | Default directory of `kontor-daemon snapshot` / `snapshots`; see [`RECOVERY.md`](../RECOVERY.md) | Generated |
| `crates/kontor-mcp/seats/*.json` | Which tier and serve profile one MCP server process runs at | See [the seats README](../crates/kontor-mcp/seats/README.md) |
| Project checkout `config/orchestration/` | `orchestration.yml`, `fleet.yml` and `teams/core-team.yml`, the authoring sources of an orchestration bundle (see below) | Sent as exact bytes to the bundle operations; the daemon never reads a checkout |
| Daemon environment `KONTOR_SEAT_MCP` | `off` disables seat MCP composition (files and creation frames) for every plane | Absent or any other value: composition on |

The state-root rows above describe the current runtime layout. Every
human-edited setting that varies by project, user or machine belongs in the
selected YAML folder instead; WP-08 migrates the remaining JSON readers and
WP-10 migrates ASMA's orchestration files, including adding the six desk
policy keys to the existing `config/orchestration/pr-desk.yml` and retiring
its `desk_folder` in favour of `project.pr_desk.history_root` plus
`host.paths.pr_desk`. Generated state and secrets stay in the state root.

Everything in the database is published through a preview/apply pair with a
content hash: the apply is compared against the hash the preview returned, so a
specification cannot change between the two.

## Experience memory transport

> **Status:** merged with ASMA-8155 in PR #291 (`abe990ac`, 2026-10-04). The
> PR records source and synthetic qualification only; live credential resolution and projection
> activation are not recorded here.

`memory-cognee.json` is optional, strict JSON, at most 8 KiB. Absence keeps
typed, bounded lexical recall enabled and performs no Cognee network work.
Only explicitly `provider_eligible` approved revisions enter the store's minimal
projection preview. The default document policy remains `local_only`.

```json
{
  "enabled": false,
  "endpoint": "http://127.0.0.1:8000",
  "credential_alias": "experience-fixture",
  "dataset_prefix": "kontor",
  "timeout_ms": 1500
}
```

| Key | Default and contract |
| --- | --- |
| `enabled` | `false`; explicit transport opt-in, separate from a document's projection policy |
| `endpoint` | `null`; self-hosted HTTP loopback or HTTPS, without userinfo, query or fragment |
| `credential_alias` | `null`; non-secret alias, never a credential value or a Jira credential |
| `dataset_prefix` | `kontor`; v1 requires this value to preserve the accepted immutable `kontor_<project_uuid>_<memory_cursor>` dataset identity |
| `timeout_ms` | `1500`; 1–1500 ms for the entire semantic operation, reserving at least 500 ms of the 2-second target for authoritative lexical selection |

The merged source qualifies **synthetic transport only**. Embeddings and
fixture tests can compose `Client::new(config, synthetic_secret)` with
`DaemonConfig::with_memory_cognee`. Ordinary daemon startup never constructs a
Cognee client: it validates the document and refuses `enabled: true` with
static `cognee_unavailable` until an authorized credential/transport
composition is supplied. It never reads Keychain, provider
credentials or environment secrets. ASMA-8159 owns live credential resolution,
release qualification and projection activation; configuration here does not
claim those capabilities are deployed.

The transport sends multipart `datasetName` and repeated `data` text files to
`/api/v1/add`, then blocking `/api/v1/cognify`, then `/api/v1/search` with
`search_type: CHUNKS`, a dataset-name filter and `top_k: 64`. CHUNKS text is
parsed only for the embedded canonical identity; backend distance is negated
into descending rank. The returned lesson/cues never supply prompt content.
Every candidate is rehydrated and frozen by the canonical store transaction.
The adapter's `qualify` method produces add/cognify/canary evidence and **does
not activate a dataset**. Live ingestion/chunk identity and release-specific
completion/dataset behavior require ASMA-8159 qualification.

Search/response work is bounded to 64 candidates and 256 KiB before decoding;
redirects, automatic retries and inherited proxies are disabled. Errors expose
static reasons without upstream bodies. An absent/stale/failed/malformed/empty
projection degrades to eligible FTS with no `list_memory` fallback. Recall
selects at most 8 whole experiences within 32768 exact canonical UTF-8 bytes,
including JSON array overhead. The general Context Pack ceiling remains 1 MiB.
Context `memory_selection.selector_version` is now 2, with `ceiling_bytes: 32768`
and `narrowed: true`. `omitted` stays empty because it cannot enumerate an
unbounded corpus; frozen recall metadata supplies exclusion counts instead.

Normal delivery root prompts append the exact frozen canonical array inside
`<experience_memory>` and identify its project, original run, result hash and
block hash. Downstream roles cite that same binding. Replays load historical
canonical bytes with hash verification before retrieving again; approval,
tombstone, task edits and projection changes do not replace the selection.
An explicit purge returns the accepted typed refusal and preserves the binding.

## Provider-unavailability markers (`runtimes.json`)

`unavailable_providers` on a runtime setting is an operational outage marker:
launch admission refuses that provider until the entry is gone. The daemon
composes the set once, at start, so an outage that lifts leaves a marker that no
restart removes by itself. The supported correction is the
`kontor_provider_marker_preview` / `kontor_provider_marker_apply` pair
(`/v1/provider-markers:preview` and `:apply`), naming the runtime family key that
holds the marker and the provider to remove.

- **Preview** is effect-free. It reads the document, reports the exact `before`
  and `after` provider sets, the document `revision`, and the effects: whether
  the correction would rewrite the document, and whether the *running* daemon
  still holds the marker (a start is required before the document governs new
  launches).
- **Apply** takes the preview's `revision` and `preview_hash` and edits only the
  addressed setting's marker array. Every other member — including fields this
  build does not know and the sensitive `host_target` — is preserved, and the
  file keeps its permissions. A document that changed since the preview is a
  `revision_conflict`. Apply holds `<state-root>/runtimes.json.lock` across
  every writer's read, comparison and rename, so two corrections cannot
  overwrite each other. A manual edit cannot take that lock: edit
  `runtimes.json` by hand only with the daemon stopped.
- **Idempotent replay.** The key is judged before the revision and claimed
  durably before the document is written: a retry of a completed correction
  answers `replayed` without a second write — even when an operator restored
  the marker after the first success — a retry whose first attempt never
  started the write finishes the correction at the revision it read, a
  correction of an already-absent marker answers `already_cleared`, and a key
  reused for anything else is an `idempotency_conflict`. An attempt that
  started writing but never recorded completion is not repeated while the
  marker still stands, because it cannot tell a write that never landed from a
  marker restored for a new outage; its retry is a `revision_conflict` asking
  for a fresh preview and key. An unknown family is `not_found`; a malformed
  document or a non-regular settings file is a typed refusal and is never
  rewritten. Completion is durable state, not a guess from the current bytes.

The correction edits one configuration member. It retires, replaces or relaunches
nothing, and no seat, run or topology node changes.

## Jira-derived backlog and topology names

An epic apply accepts `epic_backlog_code`. Omit it to allocate from the epic
title or set it explicitly when the business namespace differs from initials —
for example `KOP` for “Kontor Operational MVP”. The original assignment is immutable and
case-insensitively unique within that Kontor project. Eligible legacy-imported epics have the one-time effective-code correction described in [Native naming](NATIVE_NAMING.md#legacy-import-code-correction); original assignment evidence is preserved. Jira continues to own full
issue keys such as `ASMA-8001`. Epics created before schema v72 remain readable
without a code; reapply them through the preview/apply pair to assign one before
selecting topology v4.

Schema v74 recovers a failed create attempt in place: the original batch, create
intent and marker remain immutable, while an append-only recovery row authorizes
adoption of an exact existing issue. Recovery requires the requested Jira key,
project, issue type, epic parent, summary, description and stable marker to match
the original plan and maps results by ordinal, not connector response position.
An ordinary explicit `link` still validates key/project/type/parent without
claiming ownership of an existing issue's summary, description or workflow
status; it cannot silently replace the original create batch.

Operational topology v4 (`01936f5a-1000-7000-8000-000000000001`, revision `4`)
still validates legal hierarchy and native projection capabilities. Its
centered-dot templates are historical compatibility bytes, not current naming
authority. ASMA Operational Team Definition v1
(`01936f5a-2000-7000-8000-000000000001`, revision `1`) owns the recommended
` • ` rendering in [`NATIVE_NAMING.md`](NATIVE_NAMING.md). Migrate in this
order:

1. Preview/apply the epic graph and read back its active epic backlog code.
2. Preview/apply Jira materialization and confirm the epic and task issue
   readbacks. A requested or imported key alone is insufficient.
3. Validate/publish the Team Definition revision and preview/apply the project
   default selection under compare-and-swap. This affects future epics only.
4. Inventory explicit topics for every legacy ASW/CSW; never derive one from a
   question, title or transcript.
5. Align Kontor lifecycle with runtime-archived history through the supported
   settle, seat-retire, node-retire and node-archive operations. Only retired or
   archived nodes and inactive seats are excluded from migration; their native
   names remain historical. Never retire active work to evade a preview refusal.
6. Reconcile every legacy ticket TSW through `topology:materialize` using its
   stable historical key where available. The selected/pinned definition maps
   each open TeamRun's exact slot to one logical SeatBinding without creating or
   replacing a native session. Replay it again to prove the same binding ids.
7. Preview the existing epic's Team Definition upgrade. Confirm the complete
   identity-bound container-and-seat census before apply. Preview first
   preflights every exact slot of every live TeamRun against the target
   definition and performs no runtime read when a mapping is missing or two
   co-resident slots would render the same name.
8. Apply with one stable idempotency key. A partial result keeps the old pin and
   fences materialization; replay the same key until every exact native object
   reads back and the pin switches. The fence blocks admission, replacement,
   seat release and topology lifecycle transitions before any command write,
   logical retirement or runtime contact. The final persistence check is in the
   same immediate transaction as each seat/node lifecycle write, so lifecycle
   cannot race a frozen migration census.

Logical epic creation may freeze the selected Team Definition before step 2.
This is safe because a pin is not placement authority: every native
materialization path independently requires the active immutable epic code and
the exact confirmed Jira binding for its scope.

The corresponding `/v1` operations are `epics:preview` / `epics:apply`,
`jira:preview` / `jira:apply`, `team-definitions:validate` /
`team-definitions:publish`, `team-definition-selection:preview` /
`team-definition-selection:apply`, and `team-definition:upgrade-preview` /
`team-definition:upgrade-apply`. Historical definitions and topology revisions
are never rewritten. Placement and migration refuse before runtime mutation
when the active epic namespace, confirmed Jira binding, topic, definition pin
or exact identity readback is missing or ambiguous.

The recommended TSW `team_slots` are exactly `scope→SA`, `implement→SWE`,
`verify→QA`, and `audit→AUD`. These mappings are separate from fixed local
`slots`. A definition catalog may contain alternative-template slot ids that map
to the same role code, but a frozen TeamRun containing two slots that render the
same name is refused before runtime contact. Unknown slots are never mapped from
their spelling or logical role. In particular, Research Spike remains
unregistered until a future `SLOT_DISPLAY_NAME` revision can name its two `BA`
seats distinctly.

All seats, including ECP/ASW/CSW local slots and TSW delivery slots, resolve
through the same exact `(container kind, RoleSlotId)` lookup. The configured
role code or display label is authoritative; persisted roles and caller values
are never fallback names. Migration record and confirmation each compare the
complete live census bidirectionally by subject and immutable native identity,
so neither an omitted live object nor a stale extra target can move the pin.
TeamRun slot preflight is likewise limited to active topology: a nonterminal
run whose exact seats and node were already retired remains history and cannot
block the current pin upgrade.

Schema v77 introduced Team Definitions and migration state; v78-v80 complete
per-seat advice, receipt recovery and exact command-intent recovery. During a
v79→v80 upgrade, only a migration with a bound
`upgrade_team_definition` command receipt can recover its exact intent hash.
Any unreceipted recorded, applying or confirmed legacy migration is retained as
an explicit `legacy_unrecoverable` fence and returns a typed conflict; Kontor
never substitutes the migration fingerprint or target set for the missing
command. A deployed naming migration is therefore healthy only at schema v80
or later and only when no such recovery fence remains.

Redacted export generation 4 introduced seven Team Definition record arrays.
When reading supported generations 2 or 3, Kontor supplies those absent arrays
only as empty in-memory defaults. It removes them again for legacy canonical
hashing, continuity comparison and serialization; a genuine generation-3
export therefore verifies byte-for-byte without being rewritten into a false
generation-4 shape.

## Jira connector and credentials

`<state-root>/jira.json` connects Kontor projects to Jira. It is strict JSON
(unknown fields are refused) keyed by Kontor project id, and it never holds a
credential:

```json
{
  "schema_version": 1,
  "projects": [
    {
      "project_id": "<kontor project id>",
      "endpoint": "https://example.atlassian.net",
      "project_key": "<TRACKER-KEY>",
      "credential_alias": "<credential-alias>",
      "create_fields": { "epic": {}, "task": {} }
    }
  ]
}
```

- This native JSON file is the current reader's shape. It is a human-edited
  source and migrates to `tracker/jira.yml` in the selected config folder; the
  native file then becomes a generated projection or is removed with its reader.
- Absent means Jira is not configured. A file that cannot be read, does not
  parse, declares another `schema_version` or configures one project twice
  refuses the daemon start.
- `endpoint` must be HTTPS, or HTTP on an explicit loopback host, with no
  userinfo, query or fragment. Redirects are not followed.
- `credential_alias` is a non-secret name of at most 128 characters with no
  control characters.
- `create_fields` optionally adds the fields your Jira screens require on
  create, separately for epics and tasks: at most 64 fields and 64 KiB each,
  ids of ASCII letters, digits and `_`. Kontor keeps `project`, `issuetype`,
  `summary`, `description`, `labels` and `parent`; naming one is refused.
- `reconcile_epic_keys` optionally lists the exact Jira epic keys automatic
  reconciliation may observe and move. It defaults to empty, which means no
  epic is reconciled: adding a binding never widens what the resident
  controller writes. A repeated key is a configuration refusal. Every bound
  epic is either named here after an operator confirms it or explicitly
  excluded.

The credential lives in the OS keychain (`/usr/bin/security` on macOS, the
platform keyring elsewhere) under service
`kontor-jira:<realm-id>:<SHA-256 of the canonical state-root path>` and the
alias as account. Because the address binds the realm and its root, a copied
database under another root cannot overwrite the original realm's entry. There
is no fallback to an older host-global address: an alias enrolled before this
build must be installed again.

Install it with the daemon's stopped-realm command, piping the document from a
password manager so it never appears in argv or shell history:

```sh
# The realm must be stopped: the command takes the state-root lock.
<password-manager command> | kontor-daemon --state-root /absolute/state/root \
  install-jira-credential --alias work
# stdin: {"email": "...", "api_token": "..."}
```

The command refuses before it reads stdin unless the state root is absolute and
canonicalizable, the lock is free (`state_root_locked`), `kontor.db` is an
initialized realm at this build's exact schema (it is never created or
migrated: start the upgraded daemon once, then stop it), and `jira.json`
references the alias. The document must be exactly `email` and `api_token`,
non-blank, without control characters and at most 8,192 bytes. A write is
accepted only after the stored bytes read back exactly; on a failed write or
readback the previous entry is restored, or a new one removed, and verified.
An outcome reporting that rollback could not be proven must be reconciled
before the realm is restarted. Errors are static and never echo the input.

> **Status:** merged in PR #278 (`f95e2065`, ASMA-8015). The PR records source
> and fake-backend verification only; no installation into a real keychain is recorded here. The design and evidence are in the
> [ASMA-8015 report](../_docs/reports/2026-09-26-17-20-report-asma-8015-stopped-realm-jira-credential-kontor-operational-gap.md).

## Jira reconciliation

Kontor is authoritative for desired orchestration state; Jira remains the
external workflow system. The daemon automatically converges every task and
epic that has an exact confirmed Jira binding. No operator-triggered `jira
sync` command is required for ordinary lifecycle, gate, completion or backlog
changes.

The resident controller waits for the startup reconciliation barrier, performs
an immediate pass, reacts to committed control-plane append signals, and runs a
30-second backstop for missed notifications and restart recovery. Durable
Kontor state is the queue; there is no second in-memory desired-state ledger.
An unchanged conflict or a failed external effect waits for the bounded
backstop instead of waking an immediate retry loop.

Selection is exact and fail-closed:

- task reconciliation selects by `connector.jira`, external project, issue
  type, frozen work-profile id and frozen work-profile revision;
- epic reconciliation selects the generic epic policy and reads only epic
  completion plus child-task evidence;
- the workflow and field mapping must be installed immutable revisions in
  the same realm/project before any Jira write;
- task identity comes from the canonical task-to-Jira ledger, while epic
  identity comes from its confirmed epic binding; display item codes and
  native names are never reverse-parsed into Jira keys.

The selected `project.tracker.spec` is a YAML catalog manifest. It declares
`schema_version: 1`, `kind: jira`, the matching public `project_key`, and an
explicit list of typed YAML references:

```yaml
specs:
  - {kind: ticket_fields, path: tracker/fields.yml}
  - {kind: external_workflow, path: tracker/workflow.yml}
```

Each referenced document uses the existing `TicketFieldSpec` or
`ExternalWorkflowSpec` schema. References resolve inside the selected folder;
JSON, traversal, missing files, invalid documents, duplicate revision selectors
and conflicting external-project identities refuse startup by source. The
external project key is declared by those documents, never derived from the
public tracker key or an internal ProjectId. `specs: []` (or an omitted tracker
spec selection) offers no specs; no ASMA catalog is compiled or inherited.
Catalog loading creates no Project and installs no policy.

Installation binds the exact workflow hash and its one matching field mapping
hash in the existing Admin command and project revision transaction. Read both
catalogs back with `installed: true`. Existing identities cannot be overwritten;
a conflicting edit requires a new revision. Reconciliation reads the installed,
hash-verified bytes, including after restart or removal from the authoring
catalog. An uninstalled YAML edit never replaces governing policy. More than
one installed workflow for the same entity/frozen profile, or more than one
matching field revision, refuses as ambiguous; there is no latest-version guess.
Native connector bindings remain separate in `tracker/connectors.yml`.

Install each required workflow revision through
`connectors/{connector}/workflow-specs:install` (or the matching Kontor MCP
tool), using a fresh project revision and a stable idempotency key. A project
with high-stakes tasks and a Jira epic normally needs both the high-stakes task
revision and the generic epic revision installed. Read the catalogs
back and require `installed: true` for each exact selected revision.

Every external transition is derived from a fresh issue observation and the
currently offered destination transitions. Epic writes first persist immutable
transition authority, then apply the Jira effect, refetch the issue, and confirm
the intent only from that readback. Ambiguous or contradictory evidence is
never guessed. Conflicts are append-only, de-duplicated by subject and kind,
and stay open until an authorized explicit resolution records its receipt.

A milestone may declare an ordered `route` of exact `from` and `to` status
selectors when the external workflow cannot reach its final target in one
transition. Routes are configuration, not graph search: from the freshly
observed status Kontor selects only the one declared next destination, requires
exactly one currently offered transition to it, confirms that intermediate
destination, and then reconciles the next hop from a new observation. Every
declared status must exist in the same immutable workflow revision, every
source is unique, and each chain must terminate at that milestone's final
target without a self-edge or cycle. An undeclared, unavailable or ambiguous
step fails closed as a typed conflict; Kontor never chooses a plausible Jira
path from names or whichever transitions happen to be live.

The bundled ASMA generic epic workflow revision 2 declares the observed Jira
route for an active epic explicitly: `New (10227)` → `DRAFT (10237)` →
`TO BE GROOMED (10236)` → `Groomed (10233)` →
`READY FOR DEVELOPMENT (10213)` → `In Development (10214)`. This route was
verified from current ASMA Epic transitions and Epic changelog evidence; it is
not inferred from status wording. Installed revision 1 remains immutable for
historical readback, but new selection and installation use revision 2.

Completion continually re-evaluates child work after it leaves the ticket gate.
A task added or reopened during integration, Committee review, closeout or a
finished era returns the epic to its ticket gate under a new attributed era;
prior integration, verdict, remediation and closeout evidence remains immutable
history. Jira therefore cannot stay successfully closed over newly unfinished
child work.

## Seat supervision

Copy [`config/examples/paseo-supervision.yml`](../config/examples/paseo-supervision.yml)
to `<state-root>/supervision.yml` only when this Realm should opt into the
resident succession engine. This is the Paseo watchdog/recovery policy, not
`project.supervision`; that project key names the autonomous TPM cadence
profile. Enablement is deliberately explicit:

- with no file, no supervisor starts;
- schema v1 remains readable for legacy classification and starts no automatic
  succession;
- schema v2 requires `recovery.max_concurrent_successions`, rejects zero and
  values above the process safety bound, and starts the supervisor only when
  `watchdog.enabled` is `true`;
- a disabled schema-v2 watchdog starts no supervisor even when the concurrency
  field is present.

This prevents a daemon upgrade from silently assigning a cadence or concurrency
ceiling to an existing Realm. The shipped example selects schema v2, a
300-second cadence and five concurrent succession sagas; those are deployment
choices, not kernel defaults.

The declared normal mode is notification-first: the orchestrator yields after
dispatch and the configured orchestration surface is expected to wake it on
completion, error or permission. The watchdog is an independent bounded observer
for a turn that never completes. It may classify a suspected hang only when both
active-turn age and missing-progress evidence are stale. Recovery reconciles the
same seat first; it never duplicates a seat or cancels running work.

The `completion` block remains the orchestration policy contract; KON-OP-21 does
not add a notification transport. Its resident supervisor is the bounded
durable-recovery backstop described below, not a replacement event bus.

## Attention channel

Copy [`config/examples/attention.yml`](../config/examples/attention.yml) to
`<config-folder>/attention.yml` only when this Realm should emit attention alerts.
An absent file means Kontor emits nothing; there is no default destination.

The document mirrors the supervision policy's shape and rules:

- `schema_version` must be `1`;
- `sinks` is a non-empty list; each entry names `channel: teams_workflow` (the
  only implemented formatter), an absolute `http`/`https` URL, and bounded
  `timeout_seconds` / `max_attempts`. The list is deliberate: a second channel
  is another entry plus one formatting function (REQ-013);
- `mention.target` is required: a channel post without a mention does not
  reliably raise a phone notification. It is a comma-separated list: entries
  are trimmed, empty entries dropped, each must be one identity token (no
  whitespace, control characters, `<` or `>`, at most 256 characters),
  duplicates are removed case-insensitively, and the list holds between 1 and
  10 targets. The Teams card carries one `<at>…</at>` per target in its text
  and one `msteams.entities` mention per target;
- `emit.kinds` is a closed list of Kontor-semantic kinds. Raw agent attention
  (`permission_requested`, `turn_finished`) is refused because Paseo's native
  push already owns it. `task_needs_human` always emits, even when omitted;
- invalid documents refuse the start with a typed rule rather than a panic.

### Attention secrets in `.env`

The Workflows URL and the mention list are secrets an operator edits more often
than the policy. They may live in an optional `<state-root>/.env` instead;
[`config/examples/attention.env`](../config/examples/attention.env) is the
placeholder sample. This is a separate secret input; policy stays in the selected
YAML folder. The daemon reads the secret file only at `<state-root>/.env`; it does
not search the selected folder, the working directory or its parents. The sample
`attention.env` is never loaded as configuration.

| Key | Overrides | Value |
| --- | --- | --- |
| `KONTOR_ATTENTION_TEAMS_URL` | the `url` of the one `teams_workflow` sink | absolute `http`/`https` URL without credentials |
| `KONTOR_ATTENTION_MENTIONS` | `mention.target` | comma-separated list, same rules as `mention.target` |

- **Precedence.** A key that is set in `.env` beats the same field in
  `attention.yml`, and `attention.yml` may then omit that field (`url`, or the
  whole `mention` block). Kinds, timeouts and attempts stay in `attention.yml`.
  `KONTOR_ATTENTION_TEAMS_URL` is refused unless exactly one `teams_workflow`
  sink exists.
- **Arming.** `attention.yml` is required: `.env` is read only when
  `attention.yml` is present, so `.env` alone emits nothing. Once
  `attention.yml` exists, a field it omits must come from `.env`; if neither
  supplies the URL (or the mention list), the daemon refuses to start, by
  design.
- **File rules.** `.env` is read by the same guarded reader as other
  state-root files. It must be a regular file (a symlink, FIFO or device is
  refused without being opened), owned by the state root's owner, readable only
  by its owner (`chmod 600`; any group or other permission bit is refused,
  checked on the path and enforced again on the opened file), at most 64 KiB, UTF-8, and the same file when read as when checked. Each
  failure refuses the start with a typed rule.
- **Format.** Plain `KEY=VALUE` lines and `#` comments; a leading UTF-8 BOM and
  a shell `export ` prefix are accepted, and one pair of surrounding quotes is
  stripped from a value. Keys that do not mention `KONTOR_ATTENTION_` are
  ignored. Any other key that is not exactly one of the two above — a typo,
  another letter case, a quoted key — is refused, as is a repeated key or a
  line without `=`. No error or log prints a value.
- **Restart.** Values are read once when the daemon starts. There is no hot
  reload: restart the daemon after editing `.env`.

Delivery is one detached one-shot per alert: each configured destination gets
its own bounded `POST` sequence (per-attempt `timeout_seconds`, at most
`max_attempts`, plus backoff between attempts), the command path never awaits
it, and a failed destination is logged with its receipt id and discarded without
affecting other destinations or the command result. The waiting-seat observation class
rides the `supervision.yml` cadence; it adds no resident task.

The resident loop waits for the startup reconciliation barrier, first resumes
due durable attempts, then rebuilds its inventory on the configured cadence and
on committed append signals when `runtime_error` is configured as a wake
condition. It evaluates only nonterminal active TeamRuns and only a blocked seat
whose latest reachable runtime observation,
binding generation, account, provider quota row and immutable
runtime-observation provenance match exactly. Durable attempts are both queue
and slot lock; restart and replay resume them rather than creating another
successor. Hang classification remains read-only and is not silently converted
into quota succession.

The YAML also contains prompt paths and required skill names. Kontor validates
and exposes those references but does not reinterpret or execute them in the
resident loop. They remain orchestration-surface metadata; the selected runtime
adapter remains responsible for native inspection and placement.

> **Release status — 2026-09-05:** KON-OP-21 is implemented and merged through PR #164 (`3d7c6043`, 2026-09-04), included in the inspected release `082b63ad`. Local contract coverage is recorded; independent qualification, coherent fleet deployment and realm enablement require their own receipts and are not certified by this documentation refresh.

The release-status note above separates source coverage from independent qualification and deployment; this documentation refresh does not certify either.

## Seat permission posture

What a seat may do before it has to ask a human is declared, not inherited from
whatever the machine's harness config happens to carry. Operators write one of
three words; Kontor maps each to one internal `SeatAutonomy`.

| Written in `runtimes.json` | Means | Internally |
| --- | --- | --- |
| `autonomous` | Act within what Kontor already authorized, without asking again per tool call | `Bounded` |
| `ask` | Ask a human before each guarded action — the default, and what every seat did before this field existed | `Supervised` |
| `plan` | Read and propose, never act | `Advisory` |

`permission_posture` on a Paseo plane is a **default**, and it is resolved
most-specific-first:

1. the role slot's own `autonomy`, when the frozen team template declared one;
2. the plane's `permission_posture`;
3. `ask`.

A template that already decided keeps deciding, even when the plane default is
the wider one. A realm that declares nothing at either level behaves exactly as
it did before the field existed, which is why a generation-4 document can be read
without migrating it: absence resolves to `ask` and never widens a seat.

The bundled team pack declares the first level since ASMA-8192 (PR #227,
`edcf4bb1`): version 2 of *Plan, build and verify* and *Research spike* in
[`mvp-team-pack.json`](../crates/kontor-teams/fixtures/mvp-team-pack.json)
declares `autonomy: bounded` (`autonomous`) on every slot. Version 1 is
preserved byte for byte and declares none, so a run frozen on it still
resolves through the plane default. `bounded` stays inside the execution
authorization's window, concurrency and budget, and the destructive floor
applies under every writing posture.

### How each provider is told

Posture is translated once, by a single renderer shared between the launch and
the readback that verifies it, so what a seat is spawned under and what Kontor
later checks cannot drift apart.

| Provider | `autonomous` | `ask` | `plan` |
| --- | --- | --- | --- |
| `claude` | `bypassPermissions` | `auto` | `plan` |
| `codex` | `full-access` | `auto-review` | *refused — Codex has no read-only mode* |
| `opencode` | `build` + applied block | `build` + applied block | `plan` + applied block |
| `cursor` | `agent` | *refused* | *refused* |

Cursor is refused for `ask` and `plan` rather than mapped to its modes of those
names. Its ACP runtime permits shell writes in `plan`, and shell *and* file
writes in `ask` — the same measured finding that keeps cursor out of
consultation. A mode label is not a permission boundary, and a posture Kontor
cannot enforce is refused before launch rather than reported as held. `agent`
means what `autonomous` means, so that one stays.

OpenCode carries its posture in a permission block rather than in a mode —
`build` says nothing about what a seat may do, and `plan` is behavioural guidance
whose canary showed shell writes proceeding. It launches only when the daemon
both *can* apply that block and *says it did* for the exact agent.

### How an OpenCode seat is launched

**One `create_agent_request`, carrying everything:**

- `config.providerOptions.permission` — the rendered block;
- `config.mcpServers` — the typed seat MCP surface;
- `initialPrompt` — the first turn, a **top-level sibling of `config`**;
- `clientMessageId` — likewise top-level, derived from the launch rather than
  generated, so a retry asks about the same turn;
- `labels`, also top-level, carrying a launch-intent digest over the whole
  outgoing message *and* the prompt.

The envelope is not a guess. `CreateAgentRequestMessageSchema`
(`packages/protocol/src/messages.ts`) declares `initialPrompt`, `clientMessageId`
and `labels` as siblings of `config`; `handleCreateAgentRequest`
(`packages/server/src/server/session.ts`) destructures both from the message and
passes them to `createAgentCommand`; and the answer is a `status` frame carrying
`agent_created`, the `requestId` and the agent payload built from the live
snapshot. Read from `paseo-op20-v0.6.1-backport`, deploy pin `a07ed03e0`.

There is no second stage. An earlier design created the seat empty and sent the
first turn separately so acceptance could stand as proof; with the daemon now
reporting application on the agent itself, that turn proves nothing the snapshot
does not already say, and two effects to reconcile instead of one is pure hazard.

**Two gates, and the difference between them matters.** Before any native call,
the daemon must advertise `providerOptionsApplied`. After the create, the
returned agent must report `providerOptionsApplied: true`. The feature says the
daemon *can*; the per-agent flag says it *did*. A launch binds on the second.
Missing and `false` are both refusals.

The gate is deliberately **not** a version. Kontor shipped a version-gated path
once whose permission the daemon validated, persisted, and dropped before it
reached the provider: the v2 SDK's `promptAsync` allow-lists its body keys, and
OpenCode's own prompt route reads only `t.tools`. The version was right and the
policy never applied.

**The seat is judged from the create's own snapshot** — placement, route, and the
acknowledgement — with no follow-up fetch, because a later read answers a
question about a later moment.

### When the answer is ambiguous

A create whose answer is lost may still have landed, so it is never sent again.
Reconciliation is an exact-label paginated census on the launch intent:

| The census finds | What happens |
| --- | --- |
| one match, unbound, **and its first turn proved** | adopted — it is this launch's own effect |
| one match, unbound, first turn not provable | refused; it was created and never told anything |
| one match, already bound to a run | refused; one session may not have two owners |
| none, on a complete enumeration | `DeliveryConfirmationUnknown` — **the seat claim is kept** |
| several, or an enumeration that did not finish | quarantined: no adoption, no create, no release |

Labels prove a create happened; they do not prove the turn did. The create sends
the prompt *after* the agent exists, so an agent can carry this launch's exact
intent and never have been prompted — adopting it would seat a run on a session
that sits idle forever while the launch reports success. Recovery therefore
requires the launch's `clientMessageId` on the agent's **canonical** timeline,
scanned backward from the tail with bounded pages under one fixed epoch. An
absent id, an unfinished scan, a renumbering mid-scan, or a daemon-reported gap
all refuse.

Keeping the claim is the point, and **nothing releases it on an ambiguous
outcome** — not `agent_create_failed`, not the typed `agent_create_unresolved`,
not an unrecognised status. Releasing would let the next attempt take the slot
and create a *second* agent for a run that may already have one.

The deploy carrier (`a07ed03e0` on exact v0.6.1; `661536df9` on main) does
distinguish those two words: it records the native id before sending the initial
prompt, attempts an exact-agent archive if the create then fails, and reports
`agent_create_failed` only once that compensation is **confirmed** —
`agent_create_unresolved`, naming the agent, when it is not. The revision before
it (`a878145`) could not: the id was captured after the prompt, so a throwing
prompt left it null and a create failure was reported while the agent ran.

Kontor does not branch on the difference, and does not adopt the agent the
carrier names. Branching would make correctness depend on which build answered,
and a daemon can be rolled back under a running plane. One path — census, then
first-turn proof — serves both.

A created seat that fails any check is archived over the same socket and read
back terminal. The archive *acknowledgement* is not the cleanup: it can be lost
after the daemon has already acted, so the readback runs whether or not the send
was acknowledged, and only a fresh reading of that exact agent as terminal
counts. Live, unfetchable, or an answer about a different agent all refuse
recoverably and keep the claim. A durable bind failure
returns confirmation-unknown too: the intent label is on the agent, so
reconciliation adopts that very seat instead of stranding or duplicating it.

### Why the policy is not a file or an environment variable

OpenCode merges configuration rather than replacing it, and the layers resolve as

```text
global -> OPENCODE_CONFIG -> project -> OPENCODE_CONFIG_DIR
       -> OPENCODE_CONFIG_CONTENT -> active-org remote config
       -> managed config/preferences -> OPENCODE_PERMISSION
```

so nothing Kontor writes is the last word. Merging is per key and per nested key,
and a rule the block does not name — from an auth-backed active-org config or a
system managed profile, both of which sort late — survives and, because
permissions resolve by last match, beats the destructive floor. The create
sidesteps all of it, and the per-agent acknowledgement is what makes that
claimable rather than assumed.

> **Operational note.** A machine-global config — such as the 2026-08-22 stopgap
> some operator hosts still carry — cannot reach a Kontor-launched OpenCode seat:
> the policy is applied to the session by the daemon, not resolved from files. It
> still governs any OpenCode process started outside Kontor.

> **Status:** OpenCode and Cursor also expose an `auto_accept` per-agent feature.
> Kontor derives the intended value alongside the mode, but nothing sets it:
> the supported Paseo CLI surface exposes no `auto_accept` flag on
> `paseo agent run` or `paseo agent update`, and Kontor drives the CLI rather
> than the MCP surface where it may be settable. Check the installed Paseo
> version's CLI help rather than relying on a copied version stamp; the
> capability is read from the live surface.

## Fleet configuration (`fleet.yml`)

[`config/examples/orchestration/fleet.yml`](../config/examples/orchestration/fleet.yml)
is the neutral schema-version-2 example: placeholder accounts, synthetic model
ids and no real account. The runtime tests use
`tests/fixtures/config/fleet.yml` instead, so they pass without the owner's
fleet. The example is the successor policy; copy it into the selected config
folder under `orchestration/fleet.yml` and replace every placeholder before
publishing it.

**Current reader (state root, schema_version 1).** The live, unmigrated
`<state-root>/fleet.yml` re-reads on **every seat placement** — seat fill, seat
replacement, quota takeover, succession refresh, Committee invoke, Committee
seat recovery and Advisor invoke — so one edit changes the next placement with
no rebuild, restart or template republish. It lives outside every git checkout,
so a branch switch can never change routing. Its `domains.accounts` values are
the native account aliases that reader routes through; it has no host-route
layer.

**Successor reader (selected config folder, schema_version 2).** The selected
folder is the authoring source. The policy is previewed, published and activated
through the policy path and bound by content hash; an unactivated edit has no
live effect. The successor treats `domains.accounts` as logical slots and
resolves native aliases and provider/harness routes from `host.yml`. The neutral
example is this successor. WP-08 makes the daemon read it through the same
activation path; until then the example is a schema fixture.

> **Status:** merged in PR #271 (`7ffd650d`, ASMA-8255). The committed example is
> a parseable schema-version-2 policy, not a record of any realm's live file.

### File rules

- Current reader: `<state-root>/fleet.yml` must be a regular file, not a
  symlink, owned by the same user as the state root, and not writable by group
  or others (`chmod 600`). It is at most 256 KiB and must be UTF-8; unknown
  fields are rejected at every level, and the shipped example is a parseable
  starting point.
- Successor policy: `orchestration/fleet.yml` is a config-file reference
  relative to the selected folder, contained in it, and must exist. It is
  activated through preview/publish/activate and bound by content hash.
- The current reader's `domains.accounts` values are native account aliases. The
  successor treats them as logical slots; native aliases live in `host.yml`.
- Neither form holds credentials, tokens or environment values, and Kontor never
  logs its contents — only the hash and the name of the failing rule.

### Schema

```yaml
schema_version: 1

domains:
  claude:     { provider: claude,   accounts: [claude-a, claude-b] }
  codex:      { provider: codex,    accounts: [codex-b] }
  cursor:     { provider: cursor,   accounts: [cursor] }
  openrouter: { provider: opencode, accounts: [opencode], model_prefix: "openrouter/" }

unavailable:
  domains: [openrouter]   # a provider that does not work at all
  accounts: [claude-b] # one switched-off login

models:
  opus-5: { domain: claude, id: claude-opus-5, vendor: anthropic, efforts: [xhigh], vision: true, calibrated: true }
  sol:    { domain: codex,  id: gpt-5.6-sol,   vendor: openai,    efforts: [xhigh], vision: true, calibrated: true }

chains:
  claude-first:
    - [opus-5@xhigh]
    - [sol@xhigh]

bindings:
  team/<team_template_id>/implement: claude-first
  committee/<committee_template_id>/reviewer-a: claude-first
  advisor/<advisor_profile_id>: claude-first

rules:
  calibration_required: [team/<team_template_id>/verify]
  vision_required: [team/<team_template_id>/browser-verify]
  independent_of:
    team/<team_template_id>/verify: team/<team_template_id>/implement
```

> The state-root `fleet.yml` reader reads only `schema_version: 1` at this
> revision. The committed neutral example is `schema_version: 2`, the published
> policy successor described below; publish it through the policy operations
> rather than copying it to the state root. WP-08 makes both forms readable from
> the selected config folder.

A **domain** is one failure domain: the accounts that go down together, for
example one vendor plan. A **chain** is an ordered list of **steps**, each step
written as one domain, and each step is walked in **sub-steps**: Kontor tries
every listed model on every account of that domain before it descends to the
next step. `[opus-5@xhigh]` followed by `[sol@xhigh]` therefore means: Opus on
every Claude login, then Sol on every Codex login. Chain length has no fixed
limit; the sanity ceiling is 16 steps and 64 flattened routes.

A **binding** maps `team/<id>/<slot>`, `committee/<id>/<slot>` or `advisor/<id>`
to one chain, and applies to every version of that template or profile. A
binding wins over the frozen template chain; the template chain is used only for
seats the file does not bind. A bound chain that flattens to no admissible route
fails closed — it never falls back to the route the operator replaced.
`core/<role_code>` keys are rejected in v1.

### Models and the catalog

A route is admitted when the compiled catalog accepts it **or** the current
fleet snapshot lists its account alias, model and effort. Explicit routes —
recovery profiles, bridge moves, Committee seat recovery — pass the same check,
so a fleet-only model such as `claude-opus-5-5` is nameable without a rebuild;
`/v1/catalog` still advertises only the compiled list. Since 2026-09-05 the
compiled list also admits the approved watchdog fallback routes — `gpt-5.6-luna`
on the Codex aliases, `cursor/auto-smart`, `openrouter/z-ai/glm-5.3-flash` and
`openrouter/nvidia/nemotron-3-ultra-550b-a55b:free` (medium and high only)
(PR #238) — `cursor/gpt-5.6-sol` (PR #267), and DeepSeek V4.1 Flash as
`opencode/deepseek/deepseek-flash`, which is also the OpenCode default model;
the retired `deepseek/deepseek-v4-flash` id stays admissible for runs already
pinned to it (PR #214). Catalog presence is recognition only: headroom, role
eligibility, provider availability and the rules below still decide.

### `unavailable`

`unavailable.domains` switches off a whole domain and `unavailable.accounts` a
single alias; matching routes are dropped while a chain is flattened, so the
next placement skips them. Use it for a provider that does not work at all.
Quota exhaustion needs no entry: Kontor's quota evidence already walks past an
exhausted account. The `runtimes.json` `unavailable_providers` list still
applies on top.

### The three rules

- `rules.calibration_required` — the listed verdict seats (verify, audit, QA,
  Committee) skip every model not marked `calibrated: true`. Empty the list to
  waive calibration.
- `rules.vision_required` — the listed seats skip models with `vision: false`.
- `rules.independent_of` — a seat's routes must avoid the **vendor** the named
  seat last ran on in the same team run, for example
  `team/<id>/verify: team/<id>/implement`. If the named seat has no recorded
  route, placement proceeds and logs `fleet.independence_unknown`.

### Readback and receipts

After every load attempt Kontor rewrites `<state-root>/fleet-status.json`
(`active_hash`, `loaded_at`, `last_error`), so an edit is confirmed or rejected
at once. An invalid edit keeps the last valid snapshot, reports the failing rule
there, and is never applied in part. Every accepted version is stored once under
`<state-root>/fleet-history/<hash>.yml`, and each admitted fleet placement
appends one JSON line, including its step and sub-step, to
`<state-root>/fleet-decisions/<team_run_id>.jsonl`. Leadership effects under an
activated policy append to `fleet-decisions/leadership/<seat_binding_id>.jsonl`,
and each launch that carries fleet provenance appends what it requested and what
the runtime observed to `fleet-decisions/launches/<subject>.jsonl`; all three
are `0600`.
Rollback is one copy from `fleet-history/`.

### Turning it off, and removing a model safely

Deleting `<state-root>/fleet.yml` is the supported way to turn the feature off:
every placement then behaves exactly as if the file had never existed.

To remove a model from a live file, **remove it from every `chains` entry
first** — the validator rejects a file that deletes a model while a chain still
names it. Delete it from `models` only once no live seat is frozen on it:
explicit-route commands (bridge moves, `replace_seat` with a named route,
an Admin `model_route` on seat fill, and consultation seat recovery naming the
frozen route) re-check that exact route against the catalog when they run.
Automatic quota takeover walks the declared chain and is not affected.

Two Admin-only explicit routes move work off a provider that is down (PR #267,
operator exception of 2026-09-23). `kontor_team_run_seat_fill` accepts a
`model_route` that replaces the frozen chain for that seat; it is placed on an
eligible account with headroom before the receipt exists. `kontor_seat_replace`
with `unavailable_provider` evidence retires a provider-blocked seat, and one
that already ran also needs a `model_route` naming a different provider.
Consultation admits the exact `cursor/gpt-5.6-sol` `xhigh` route only under an
operator-accepted recovery profile, never from template data; Cursor `plan`
refuses direct file tools, not shell writes, so this is a recorded risk
acceptance.

### Activated fleet policy (ASMA-8280)

> **Status:** merged with the shared orchestration runtime in PR #292
> (`4097afb8`, ASMA-8278, 2026-10-04). No realm activation record is kept in
> this repository.

The policy both orchestration modes share is authored in the project checkout
and handed to Kontor through four admin operations, which the CLI and MCP
registry serve from the same route table:

| Operation | Effect |
| --- | --- |
| `kontor_fleet_policy_get` | Which source decides routing (`activation` or `fleet_yml`), the activation record, the hash placement reads now, and why an activation cannot be served. |
| `kontor_fleet_policy_preview` | Validates one document (schema_version 1 or 2) and returns its content hash. Writes nothing. |
| `kontor_fleet_policy_publish` | Writes the previewed bytes, unchanged, to `fleet-history/<hash>.yml`. **Selects nothing.** |
| `kontor_fleet_policy_activate` | Atomically replaces the owner-only `fleet-activation.json` with the named published hash, fenced on `expected_active_policy_hash`. |

Once a record exists, placement reads exactly the artifact it names, and the
record and artifact are both re-verified on every read: file rules, content
hash and schema. Editing `fleet.yml`, a checkout, or publishing without
activating has no effect. A record or artifact that fails verification refuses
every placement that resolves a fleet binding, naming the failed check; it
never falls back to `fleet.yml` or to template routing. Deleting the record
returns the Realm to the unmigrated `fleet.yml` behaviour above. Rollback is
activating a recorded hash.

A published policy names accounts, models and seats; it never locates or
authenticates them. Preview and publish refuse any value that carries
credential material, a filesystem path (such as a provider home) or an email
address (V-33); the unmigrated `fleet.yml` reader keeps its v1 checks.

Every reader takes the same activation decision from `kontor-fleet`: only the
bytes at the record's content address, validating under the record's schema,
are the policy. A reader outside the daemon that follows the record's two files
therefore resolves exactly what placement resolves, and its choice for a seat —
the first route in chain order that the stated eligibility admits, with the
reason every other route was passed over — is the same record for the same
policy bytes, key and eligibility.

Schema version 2 keeps every v1 section and rule and adds one binding family,
`leadership/<core-team-revision-hash>/<role-slot-id>`, for epic leadership
seats. The hash is the canonical content hash of the epic's pinned Core Team
revision, so a roster change is a new key. `leadership/lsa`,
`leadership/<slot>` and `core/<role_code>` are rejected. For a bound LSA or TPM
seat, Core Team materialization, route correction and launch-intent
supersession refuse any caller route the bound chain does not offer, and each
admitted leadership effect appends its policy hash, binding, chain position and
occupancy generation to `fleet-decisions/leadership/<seat_binding_id>.jsonl`.
A leadership seat no policy binds keeps its caller-supplied route.

A materialization route may name `eligibility`
(`{unavailable_accounts, excluded_vendors}`) instead of `model_route`. The
activated policy then chooses: the first route in chain order the stated
eligibility admits, the same choice a direct-mode reader makes, and the
decision records that eligibility. A named `model_route` is admitted or refused,
never replaced. A route naming both or neither is invalid; a seat no policy
binds, or whose bound routes are all ineligible, is refused before any seat
exists.

Governed delivery and consultation seats a fleet policy binds are chosen the
same way. The exact quota observation is first stated as an explicit
eligibility — an account alias with no account whose headroom admits a new seat
now is unavailable, and a seat under `rules.independent_of` excludes the vendor
its partner ran on — and the shared resolver (or, for a Committee, the shared
allocator) chooses under it. The scheduler's rules still decide whether that
choice launches now, waits for a near reset on a route it would rather have, or
escalates. The eligibility is recorded with the decision: on each
`fleet-decisions/<team_run_id>.jsonl` line, on a fleet-routed Committee or
Advisor admission, and on a fleet-routed Committee seat recovery profile. A
seat with no fleet binding, and an explicitly named caller route, walk the
unchanged headroom chain and record no eligibility.

#### Aligned activation: the orchestration bundle

A schema_version 2 activation record names one orchestration bundle as well as
its policy. The bundle is authored in `config/orchestration/`:
`orchestration.yml` (`schema_version: 1`, `fleet: fleet.yml`,
`core_team: teams/core-team.yml`) selects the policy and the explicit Core Team
source. `teams/core-team.yml` pins one role catalog by `catalog_id`, `version`
and `content_hash`, and declares every seat in order — including the selected
domain's required purpose owners — with its `role_slot_id`, `role_code`, `presence` and
`ad_hoc_allowed`. The publisher resolves it through the Core Team resolver, and
the canonical revision bytes it produces define the `core_team_revision_hash`
every `leadership/<hash>/<slot>` binding names.

Five admin operations, served by the route table, the MCP registry and the CLI
alike, carry a bundle from proposal to activation:

| Operation | Effect |
| --- | --- |
| `kontor_fleet_bundle_propose` | An initial `orchestration.yml` and `teams/core-team.yml` holding only the mandatory roles, with the exact catalog revision (id, version, content hash) they pin. Writes and activates nothing; nothing reads the proposal until it is published and activated. |
| `kontor_fleet_bundle_preview` | Resolves the exact `orchestration`, `fleet` and `core_team` bytes against the realm catalog revision the Core Team source pins, and returns the manifest publication would write and the preview hash, which binds those bytes and that catalog revision. Writes nothing. |
| `kontor_fleet_bundle_publish` | Resolves the same bytes again, requires that preview hash, and writes and verifies the immutable policy, roster, selected domain artifact and manifest. **Selects nothing.** |
| `kontor_fleet_bundle_activate` | Names `source_bundle_hash` and `expected_active` — the standing record's `policy_hash`, plus its `source_bundle_hash` when it is a v2 record, omitted entirely only when no record stands — verifies every artifact, and replaces the one pointer atomically. |
| `kontor_fleet_bundle_get` | The record as a bundle: `activation_schema_version` 1 or 2, the record, and the manifest a v2 record names. |

Publish and activate keys are bound realm-wide, each to its complete logical
request. The single-policy operations above still work unchanged and write a
v1 record over the same pointer.

Publication writes three immutable artifacts: the policy under
`fleet-history/`, the canonical roster under
`core-team-history/<core-team-revision-hash>.json`, and a canonical manifest
under `orchestration-history/<source-bundle-hash>.json` recording the resolver,
the hash of each source file, the policy hash and schema, the role-catalog pin
and the roster hash. Activation verifies every artifact first and then replaces
the one pointer. A selected pack adds `operational_domain: {source, source_hash}`
to the manifest, with an identical `sources[source]` entry. `source` is the
selected-folder-relative YAML reference; readers use the content address
`orchestration-history/domains/<source_hash>.yml`. The existing publisher stores
the exact captured bytes and verifies their typed domain, catalog and frozen
roster purposes before exposing the manifest. Format-only, prompt-only and
delivery-only edits change source/bundle identity and the publication preview.
This immutable artifact is generated publication output, not editable authoring
or a rollback backup. It creates no Project and activates nothing.

Every bundle read and activation verifies the artifact's file guards, exact
byte hash and cross-references. Missing/tampered artifacts refuse by name;
republishing an existing manifest cannot repair it from current YAML. Historical
manifests without the optional descriptor retain their original canonical bytes
and remain readable. A domain-dependent reader refuses
`operational_domain_unpinned`; it never infers a domain from the catalog,
compiled data or the current selected folder. There is no second active pointer.

The activation record remains:

```json
{
  "schema_version": 2,
  "source_bundle_hash": "<manifest hash>",
  "policy_hash": "<policy hash>",
  "policy_schema_version": 2,
  "core_team_revision_hash": "<roster hash>",
  "activated_at": "<timestamp>"
}
```

Readers verify the pointer, then the manifest (which must agree with it), then
exactly the policy, roster and optional domain artifact it names, and that the roster was resolved against
the catalog the manifest pins. Under an aligned activation a governed leadership
launch consumes only the selected roster: an epic whose pinned revision is not
those exact bytes keeps its pin, is never retargeted, and its leadership launch
is refused. A schema_version 1 record keeps its exact shape and behaviour and
selects no roster.

Core Team preview/apply hashes bind the complete resolved roster, including
its exact ordered seat metadata and purposes, in addition to version, catalog,
selected raw pack source hashes and effects. The apply intent names that
authorization hash. The exact catalog and resolved seats publish atomically;
unknown or non-current roles and conflicting immutable catalog bytes refuse.
Promotion binds the complete frozen roster; an explicit roster upgrade binds
both the exact source and target rosters and the epic-side revision. An effects
summary alone cannot authorize replacing metadata under the same version and
catalog. A stale preview refuses before publication or effects.

A project's Core Team can be published from a published bundle through the
existing `kontor_core_team_preview` and `kontor_core_team_apply`: name
`source_bundle_hash` instead of `seats` (exactly one of the two). Both steps
re-verify the bundle's immutable artifacts and publish its frozen resolved
roster byte for byte, including purposes. They never rederive purposes from a
later delivery edit. The bundle must be authored at the project's next Core Team
version against the catalog the realm holds, or the preview refuses. The apply
names the bundle in its intent, receipt and outcome. Nothing else changes: no
epic pin or running seat is retargeted, and an explicit-seat request is
unchanged. Bundle manifests also pin the four selected pack sources used during
resolution; an unactivated authoring edit cannot replace those immutable bytes.

#### Direct-mode resolution without a daemon

`kontor --state-root <root> --tier operator fleet-policy-resolve --binding-key <key>`
(optionally `--unavailable-accounts '[...]'` and `--excluded-vendors '[...]'`) is
the registry's one local operation. It is operator work, which admin inherits;
an observer is refused. It runs in the CLI before any connection — no daemon,
credential file, base URL or network — through the same verified reader the
daemon uses, and prints `{tool, status: 200, body: FleetSelection}`. Nothing
eligible prints the selection under `status: 409`, `placement_blocked`.
Anything missing, unsafe, mismatched or malformed is a local refusal naming its
rule; there is no `fleet.yml` and no last-valid fallback, and a schema_version 1
activation resolves no leadership key. MCP neither lists nor dispatches it: it
has no `/v1` route.

An empty `--unavailable-accounts` set is not evidence that a route is
available. A caller that holds positive evidence states it in single mode with
`--route-health`:

```json
{
  "as_of": "2026-10-25T09:00:00Z",
  "max_age_seconds": 60,
  "mode": "default",
  "observations": [
    {"account": "<account>", "model": "<model>", "effort": "xhigh", "mode": "default",
     "state": "available", "observed_at": "2026-10-25T08:59:30Z", "revision": "<host read>"}
  ]
}
```

With route health stated, a route the eligibility admits is still passed over
unless exactly one observation of its account, model, effort and `mode` says
`available` and is at most `max_age_seconds` old at `as_of`. The exclusion
reasons are `route_unobserved` (no observation), `route_health_unknown` (the
read could not tell, several observations cover the route, or the one that
does is dated after `as_of`), `route_health_stale` and
`route_unavailable_now`. `as_of` is stated, never read from a clock, and the
selection echoes the route health so the receipt reproduces the choice.
`max_age_seconds` runs from 1 to 3600 (H-02). Route health belongs to single
mode; `--allocation` and `--planning-pair` refuse it (H-01). Without it, the
selection is negative-only as before.

The same operation allocates a whole Committee jointly when it is given
`--allocation` instead of `--binding-key` — exactly one of the two:

```json
{
  "diversity": "distinct_vendor_per_reviewer",
  "slots": [
    {"slot_id": "reviewer-a", "role": "reviewer", "binding_key": "committee/<template>/reviewer-a"},
    {"slot_id": "reviewer-b", "role": "reviewer", "binding_key": "committee/<template>/reviewer-b",
     "unavailable_accounts": ["codex-b"]},
    {"slot_id": "judge", "role": "judge", "binding_key": "committee/<template>/judge"}
  ]
}
```

Each slot states its own eligibility; the top-level `--unavailable-accounts`
and `--excluded-vendors` belong to single mode. Slot ids are unique. The
activation is loaded and verified once, every slot is resolved against that
snapshot, and the allocation is the shared allocator's: slots in the order
given, then each chain in policy order; no two reviewers share a policy vendor,
and a reviewer route whose vendor is `unknown` is not eligible; the judge is
held only to its own eligibility. The answer is one complete ordered allocation
under one shared provenance (policy, and for an aligned activation bundle and
roster) with each slot's binding, chain, eligibility, every candidate
considered and why it was passed over, and the chosen step, sub-step and vendor
— or, under `status: 409` `placement_blocked`, the same receipt with no slot
selected and the reason. There is no partial allocation. Governed Committee
admission calls the same allocator.

The same operation places one `planning_pair@1` pair when it is given
`--planning-pair` instead. Name exactly one of `--binding-key`, `--allocation`
and `--planning-pair`:

```json
{
  "members": [
    {"slot": "seat-a", "binding_key": "advisor/<profile-a>"},
    {"slot": "seat-b", "binding_key": "advisor/<profile-b>",
     "unavailable_accounts": ["codex-b"]}
  ]
}
```

The members are exactly `seat-a` then `seat-b` (PP-01). Each names an existing
binding and states its own eligibility. The top-level `--unavailable-accounts`
and `--excluded-vendors` belong to single mode (PP-04). Combining
`--planning-pair` with another mode is refused (PP-03); `--binding-key` with
`--allocation`, or no mode at all, is J-01 as before. The pair has no role,
Judge or diversity to set. It is one joint allocation of both members through
the same allocator under `distinct_vendor_per_reviewer`: the members never
share a policy vendor, and a route whose vendor is `unknown` is not placed. The
answer is `{protocol: "planning_pair@1", selection, placement_hash, members}`:
`selection` is the joint allocation receipt, `placement_hash` its canonical
hash, and `members` the two frozen members. When no such placement exists, the
answer is the same document without `members` under `status: 409`
`placement_blocked`. It is placement evidence only: it carries no verdict, and
it never satisfies a review gate.

Placement is not launch. At `0971674d` the Paseo adapter refuses every
planning-pair member route before any native effect, with
`PlanningPairMemberSurfaceUnsupported` naming the provider and its gap
(`../crates/kontor-runtime-paseo/src/adapter.rs`), so no planning pair launches
through the production Paseo runtime. See
[`CONSULTATION_LIFECYCLE.md`](CONSULTATION_LIFECYCLE.md#governed-planning-pairs-asma-8282).

## Provider quota signals

[`config/examples/quota-signals.yml`](../config/examples/quota-signals.yml) is
the neutral example: its markers are deliberately fake and match no live vendor
copy. The parser tests use `tests/fixtures/config/quota-signals.yml`. Copy the
example to `<state-root>/quota-signals.yml` (and, after WP-08, into the selected
config folder) and replace every marker with captured vendor wording. The sentences are data on purpose: a vendor rewords its
message far more often than Kontor ships, and encoding them as Rust constants
would make tracking a copy change a rebuild.

Install and read it back:

```sh
cp config/examples/quota-signals.yml "$KONTOR_STATE_ROOT/quota-signals.yml"
$EDITOR "$KONTOR_STATE_ROOT/quota-signals.yml"
# Readback: the daemon refuses to start on a present-but-invalid document, so a
# clean start is the readback. Confirm what it now classifies with:
kontor --state-root "$KONTOR_STATE_ROOT" provider-quota-states-list
```

Each signal carries the provider as the catalog spells it, whether the vendor
charges a plan allowance or a prepaid credit balance, the markers that must all
appear before text is read as a refusal, and — for a plan allowance that states
one — the text preceding the reset instant and the IANA zone a bare wall clock
is printed in. A vendor that prints local time without naming a zone cannot be
read correctly without that field, and guessing wrong shifts the reset by hours.

**Every signal carries an identity, and the alias is not it.** Each entry
declares a stable logical `id`, unique within the document, and a positive
`version` that increments whenever its wording or parsing changes. Two logins of
one vendor carry the same sentence under the same family, so a record naming
only `claude-b` could not say which fingerprint authorized a retirement —
which is why the shipped Claude entries have distinct ids despite identical
wording. A signal's complete definition (id, version, provider, basis, ordered
markers, reset prefix, zone) is digested, and durable provenance cites that
digest: changing any of it under an unchanged id and version produces a
different digest, which immutable history is entitled to refuse.

**`provider` is an exact catalog alias, never a vendor family.** A deployment
addresses one login per alias — `codex-b` and `codex-a` are two
accounts of the same vendor — and each account's routing document declares
exactly which aliases it may select. A quota state is keyed by
`(account, provider)`, so a signal naming the bare family `codex` matches no
account that routes `codex-b`, and classification for that account is
silently inert. **Name one entry per alias**, repeating the vendor's wording as
many times as the deployment has logins.

**Order is significant, and eligibility is applied first.** The daemon filters
this sequence to the aliases the seat's own account may select, and only then
reads the text; classification returns the first *eligible* signal whose markers
all appear. So repeating identical wording across two aliases is safe —
`codex-b`'s entry can never stand in front of `codex-a`'s for a seat
running on the `codex-a` login. Order still decides between two entries that are
both eligible for one account, which is why the shipped example lists the Claude
aliases before the Codex ones: the whole of the Codex marker set is the words
"usage limit", which a Claude refusal also contains.

**A vendor that restates its zone is checked against the declared one.** Some
messages print `… resets 10:40pm (Europe/Chisinau)`. That annotation is never
read as part of the clock, and it is **compared** rather than skipped: *every*
parenthesized token in the message is checked, and any that names a zone the
tzdb knows must **agree** with `reset_zone`. A disagreement anywhere — including
one hidden behind an earlier unrecognised annotation such as `(EEST)` — yields
no instant at all — the account still blocks, as `Unknown`, which is the
visible prompt to fix the signal. Ignoring it would let a message saying
`(Europe/Oslo)` be converted as Chisinau and land an hour wrong with nothing to
show for it. An abbreviation like `(EEST)` is not an IANA name, cannot be
compared, and is left alone.

**A stated zone is the provenance of a captured message, not the host's
clock.** `reset_zone` qualifies the wall clock *that vendor's message printed*,
recorded alongside the wording it belongs to. It is never inferred from the
daemon's own timezone: a host that later moves to another zone is a fact about
now, and letting it reinterpret a historical fingerprint would silently move
every reset derived from it. The shipped Codex entry states `Europe/Oslo`
because that is where the 2026-08-21/23 incident message was captured, and the
Claude entries state `Europe/Chisinau` because that is what their own
2026-08-30 message printed — neither because any particular machine runs
there.

**Only an exact, distinctive system-refusal fingerprint may activate a signal.**
A bare phrase like `usage limit` is not sufficient: an ordinary assistant
message *discussing* limit handling contains it, and this configuration has the
authority to archive a live seat. Require the vendor's framing, its settings
URL and its retry wording together. A vendor whose refusal has not been captured
stays commented out rather than shipped on unverified copy — a false negative
falls back to the poll and the operator, while a false positive retires work
that was running.

**Absent, unreadable and invalid are three different outcomes.** Only the first
is inert:

| The document is… | Kontor… |
| --- | --- |
| absent | leaves refusal-message classification inert; pre-flight provider polling is unchanged, and a schema-v2 supervisor can resume existing durable attempts but cannot derive a new quota decision from unconfigured wording |
| present but unreadable | refuses to start, with a typed `Read` naming the path |
| present but unparsable or schema-invalid | refuses to start, with a typed `Document` or `Invalid` naming the stable rule |

A broken document is never quietly degraded to "inert". It states an intent the
realm cannot honour, and starting anyway would leave an operator believing
reactive classification is armed when it is not.

> **Status:** both Claude entries match the refusal captured verbatim on
> 2026-08-30 (quoted in the example) and declare `reset_prefix`
> `'your session limit resets '` with `Europe/Chisinau` (`2087430e`,
> `f80446bc`); the earlier provisional, prefix-less Claude entry is superseded.
> Both Codex entries are verified against the message recorded on 2026-08-21.
> The `opencode` entry reads a prepaid `credit_balance` refusal, which records
> as drained with no reset instant.

## Seat MCP surface

A Kontor MCP server process holds exactly one credential tier and therefore *is*
one seat; running at two authorities means running two servers. Within that tier
a **serve profile** may narrow which tools the server lists and admits — never
widen it, and never beyond what the credential already allows.

| Seat file | Tier | Serve profile |
| --- | --- | --- |
| `paseo-lead.json` | `admin` | none — the whole vocabulary |
| `worker.json` | `operator` | `worker` — 25 tools: read its work, claim, settle and observe a turn, record an artifact and a gate verdict, session follow-up, intake, memory search/history/propose, experience proposals and recall previews, resolve context, list and record open questions |
| `reviewer.json` | `observer` | none — reads only |

Profiles are declared in the registry beside the tier declarations, deliberately
not in the seat file: a free-form tool list in configuration would be a second
authority model that drifts. An unknown profile name refuses to start, and a tool
the profile excludes is refused at call time as well as hidden from the list. See
[`../crates/kontor-mcp/seats/README.md`](../crates/kontor-mcp/seats/README.md).

The daemon composes its own seats' servers from the same registry
(`../crates/kontor-runtime-paseo/src/seat_mcp.rs`). Every composed server runs
`kontor-mcp --credential-tier operator --serve-profile <profile>`; a scoped
consultation or hosted credential reaches it only through `KONTOR_AUTH` in the
creation frame's secret channel, never through a shared file.

**The seat kind selects the profile.** Every admitted delivery seat composes
`worker`; every Advisor and Committee seat composes `consultation`, and never the
worker or admin surface. Claude receives the profile in its worktree `.mcp.json`;
Codex, Cursor and OpenCode receive it in the creation frame. The remaining
composed kinds keep their own profiles:

| Profile | Tools | Composed for |
| --- | --- | --- |
| `worker` | 25, as above | **Delivery seats**: Claude in the worktree `.mcp.json`; Codex, Cursor and OpenCode in the creation frame |
| `consultation` | 5: Advisor run read and settle, Committee run and artifact read, findings record | **Advisor and Committee seats**, with the consultation tool guard: Claude in the worktree `.mcp.json`; Codex, Cursor and OpenCode in the creation frame |
| `planning_pair_member` | 3: run read, findings record, answer record | planning-pair members, with the guard — refused before launch through Paseo today ([above](#direct-mode-resolution-without-a-daemon)) |
| `planning_pair_caller` | 4: run read, invoke, clarification request, disposition record | nothing; selectable by name only |
| `leadership` | 6: completion read and remediation, open-question list and record, Committee-seat permission inspection and response | hosted LSA, TPM and desk seats: Claude's cwd files plus the creation frame for every provider |

A leadership permission response accepts any valid `Idempotency-Key`:
non-empty, at most 256 characters, with no surrounding whitespace, control
characters or secret-shaped text. A canonical UUIDv7 is the convention, not a
requirement. A key that is one becomes the response id, and any other key
derives a stable one, so a retry with the same key is still answered from the
ledger. The response is persisted in schema v75 before the runtime effect.
Confirmed replay is inert; confirmation-unknown dispatch fails closed instead of
guessing or answering a second time.

A Claude hosted desk seat whose proved workspace is a local directory rather
than a Git worktree gets its `.claude/settings.local.json` then `.mcp.json`
published into that directory, with the exact originals kept in a private
journal at
`<state-root>/hosted-directory-publications/<sha256(cwd)>.json` and recovered
under a canonical-cwd lock before any create or reload; a healthy replay only
validates, and refuses when the files differ (PR #300, `0971674d`). Codex,
OpenCode and every other provider receive the leadership MCP only in their
creation frame, which a Claude seat receives as well. Delivery and consultation
seats never use this path. `KONTOR_SEAT_MCP=off` in the daemon's environment,
read once at daemon start, disables further seat MCP composition
for every plane; files already composed into a worktree or hosted directory and
sessions already running need separate cleanup. With `off`, creation frames
carry no Kontor MCP either, and Claude Advisor, Committee and planning-pair
member launches are refused. Declared permission posture is composed either way.

Quota succession is exposed separately. `kontor_seat_quota_states_list` is an
Observer read joining each live delivery seat to its exact account and provider
quota projections. `kontor_seat_recover` is an Admin, bodyless command addressed
only by project, predecessor run and `Idempotency-Key`; the server fresh-reads
and freezes every binding, revision and quota-provenance fact rather than
accepting an eligibility claim from the caller.

## Durable desks, desk seats and PR reviews

> **Status:** merged in PRs #295 (`733807cc`), #296 (`430975a3`), #297
> (`b3db4adf`), #298 (`e376f405`), #299 (`813135fd`) and #300 (`0971674d`),
> 2026-10-11. The
> [qualification report](../_docs/reports/2026-10-11-16-00-report-asma-8454-desk-qualification.md)
> records a frozen source candidate with Independent Review and release
> readback pending.

Desks are Team Definition data, not a file in the state root. What configures
them:

- **Which desks exist.** A Team Definition revision's `desks` array, composed
  against a topology revision that declares the `DESK` and `DWS` kinds. The
  bundled default declares none, and nothing selects a desk-capable revision
  implicitly: the shipped ASMA Operational Team Definition v7 (`adam`,
  `pr-review`) takes an explicit selection preview and apply, then a
  materialization per desk.
- **Desk Adams and PR reviews.** A revision opts in with
  `desk_leadership {desk_key, epic_kind, ad_hoc_kind}` and
  `pr_review {desk_key, adam_kind, room_kind, room_roles}`. No bundled revision
  declares either, so a project publishes its own successor first.
- **Durable seats.** Each is an immutable dated declaration through
  `kontor_desk_seat_declare` (Operator). `recovery_owner: paseo_direct` is
  recorded and reported only; `daemon` makes Kontor launch and recover it as a
  hosted seat and needs one exact compiled-catalog `model_route`. To amend a
  decision, declare a new `seat_key`.
- **PR review repository authority** comes from the confirmed Jira key's
  publication binding, never from a title.
- **Where they run.** A desk registers `runtime-roots/<project>/desk-<desk node>`
  and a review room `runtime-roots/<project>/desk-<desk node>-room-<room node>`;
  hosted desk seats get the [leadership surface](#seat-mcp-surface).

Operations, refusals, backup and rollback are in the references:
[durable desks](../_docs/reference/2026-10-10-23-55-reference-kontord-durable-desks.md),
[desk seats](../_docs/reference/2026-10-11-02-20-reference-kontord-durable-desk-seats.md),
[desk Adams](../_docs/reference/2026-10-11-05-36-reference-kontord-desk-adams.md) and
[PR reviews](../_docs/reference/2026-10-11-11-35-reference-kontord-pr-reviews.md).

## GitHub App for publication identity (ASMA-8101)

Kontor judges every publication against the confirmed Kontor/Jira binding
(`publication:preview` / `publication:attest`). The forge learns the verdict
only through a GitHub App whose identity cannot bypass the rules it reports on.
The App is optional: without it the attestation endpoints still serve the ASMA
CLI, and nothing is posted to GitHub.

> **Status:** merged in PR #195 (`e4bb5fbd`). Whether an App is installed and
> configured for a realm is not recorded here.

Create the App once per organisation (Settings → Developer settings → GitHub
Apps → New GitHub App) with:

- name `<publication-app-name>`; webhooks disabled (Kontor polls);
- repository permissions: **Checks: read and write**, **Pull requests: read**,
  **Contents: read and write** (the squash merge), **Metadata: read**;
- install it on exactly the governed repositories;
- generate one private key and store the PEM outside every repository, mode
  `0600`, for example `<state-root>/config/github-app.pem`.

The successor authoring source is `forge/github-app.yml` in the selected config
folder; the committed placeholder is
[`config/examples/forge/github-app.example.yml`](../config/examples/forge/github-app.example.yml):

```yaml
schema_version: 1
app_id: 123456
installation_id: 654321
private_key_path: <absolute-path-outside-every-repository>/github-app.pem
project_id: <kontor-project-id>
repositories: [<owner>/<repository>]
check_name: <check-run-name>
poll_seconds: 60
```

`app_id` and `installation_id` are integers; replace the numeric placeholders
with the App's own ids. The PEM path is a reference; the key itself stays
outside every repository. Today the native `<state root>/config/github-app.json`
file is a human-edited source. It migrates to `forge/github-app.yml`, with the
PEM path as a reference; the JSON file then becomes a generated projection with
provenance.

For selected-folder startup, `project.check_runs` must contain exactly one
`publication-identity` name, either unprefixed or ending in
`/publication-identity`. The gateway uses that exact name. The optional App
`check_name` may repeat it; a different name, a missing project identity check,
or multiple matching entries refuses `forge/github-app.yml` before key access.
An absent App document remains valid and does not require an identity check.
The explicit embedded native reader retains its legacy default; selected YAML
never inherits that default.

`poll_seconds` defaults to 60 (at least 1).
An optional `api_base` (default `https://api.github.com`) points
the gateway at another GitHub API host. `repositories` must list at least one
`owner/name`.

Absence of the document is valid. A present document that will not parse, or
that names a key that will not load, refuses the start: an operator who wrote
it believes the forge check is armed.

With the document in place the daemon:

- polls every open pull request of each listed repository every
  `poll_seconds`, judges its head branch, head commit and title through the same
  attestation the CLI uses, and posts one completed check run named
  `check_name` per judged head/title (`success` or `failure`, with the stable
  reason codes in the summary);
- serves `publication:merge`, which re-reads the pull request, refuses a head
  that moved since it was judged (`head_sha_stale`), re-judges, and squash-merges
  through the App identity bound to that exact head.

Make the check required on the default branch through a repository ruleset
(`required_status_checks` with `integration_id` set to the App id) so a status of
the same name posted by anyone else does not satisfy it. On GitHub Team plans a
branch-name ruleset is not evaluated; this check is the forge-side identity gate.

The repository also ships the dependency-free publication branch/title Actions
check. Require the check name selected by the projection's `project.check_runs`
(the entry ending in `publication-branch-title`; the committed projection
currently selects `asma/publication-branch-title`) on the default branch as the
forge-local grammar boundary: it rejects a PR unless the base is the project's
configured default branch, the branch is canonical, and branch and title carry
the same Jira key. The project key, base branch and check name come from the
selected YAML config folder; the trusted-base workflow reads a committed
projection or the repository variables generated from that folder, and opens a
Checks API run named by that selected value. The selected check name is used
before the other variables are validated, so a partial variable set completes
that name as failure; if no check name is resolvable, the committed projection's
name is failed. Require both the configured API check and the static
`Publication branch and title identity` job check, so an unresolvable name
cannot leave an earlier required API check green.
A `dependabot/` branch opened by `dependabot[bot]` is exempt from the branch
and title rules (code `bot_branch_exempt`); the base and head checks still
apply. The GitHub App check remains the stronger semantic boundary
because it also proves that key against Kontor's confirmed Jira binding.

## Other deployment data

- Profile packs define phases, gates, artifacts, budgets and runtime routing.
  The bundled manifest declares 17 work-profile categories; four ship today
  (`code`, `ux-ui-layout`, `research`, `docs`).
- Team Definition JSON revisions define native hierarchy, naming, fixed slots,
  delivery `team_slots`, exact labels and slot capability-profile references.
  Team templates and
  consultation profiles separately own execution behavior, skills, context and
  handoffs. Role slots carry stable ids, so two peers in the same role are
  explicit rather than duplicate.
- The standard role catalog defines 56 role codes across 9 segments. Seat
  selection is by `role_code`; a free-form role string is not accepted anywhere.
- Account profiles contain non-secret provider-routing metadata and a credential
  reference. No surface — DTO, row, log, export or process argument — has a field
  for a secret value.
- Completion profiles name the integration team, the verdict committee, the
  number of remediation rounds and an optional polling fallback. The seeded
  `operational_default` allows one remediation round.
- Native container and seat naming is rendered only from the pinned Team
  Definition revision; callers and adapters do not improvise it.

Changing a prompt, duration, template or specification changes configuration.
Changing a safety invariant requires an architectural decision and code review.

### Fleet-origin consultation permission policy

A schema-version-2 fleet policy may declare `consultation_permissions` as an
ordered list of exact `{account, model, effort, mode}` records. `account` is an
exact member of the model's domain, `model` is its symbolic `models` key,
`effort` is one declared supported effort (explicit `null` only for a model
without efforts), and `mode` is exactly `plan`. Each record applies only to a
Cursor or OpenCode consultation route with a known vendor. Its model must
explicitly declare `permission_eligible: true`; the optional boolean defaults to
false, including automatic selectors such as `auto-smart`. Eligibility alone
grants no permission. A permission-bearing historical policy without this field
refuses validation; policies without permissions remain readable. These exact
bytes, including eligibility, participate in the existing policy hash.
Unknown, partial,
wildcard and duplicate records fail fleet validation; omission grants no
fleet-origin exception. This is behavioral risk acceptance, not filesystem
containment, and does not relax other admission or readback gates.

The existing fleet preview/publish/activate path governs these bytes. Admission
freezes its existing fleet policy hash in the consultation context. Launch and
retry verify the immutable `fleet-history/<policy-hash>.yml` artifact and match
that exact hash, account, native model id and effort. Current authoring,
unactivated publications, later activations and restart cannot add permissions
to a frozen run. Missing or tampered artifacts refuse; they are never repaired
from authoring. No new table, active pointer or editable projection is added.
Ordinary Claude and Codex modes are unchanged.

Recovery profiles keep their own canonical evidence hash and route source.
New native replacement and native-less materialization recovery also freeze
`consultation_permission_policy: {source: fleet_configuration, policy_hash: ...}`
in the existing canonical profile document before effects. This is the exact
verified activated policy used to prepare that recovery, independently of
whether a fleet chain selected its route. It is not an API permission knob.
Cursor/OpenCode effects require an exact allowance in that immutable artifact.
Pending recovery reads the recorded policy, never current authoring or activation;
restart, later activation and a matching current fleet route cannot replace it.
A conflicting route-policy hash, malformed source, absent/tampered artifact or
missing allowance refuses before native retirement or launch.

Historical profile-only rows remain readable and existing committed receipts
remain replayable without new effects. Their profile hash, constraint policy hash
or incidental fleet match is not a permission pin. They receive no backfill and
cannot grant a new Cursor/OpenCode launch or replacement; missing permission
metadata refuses as `consultation_permission_unpinned`. Ordinary provider modes,
recovery fencing, whole-Committee constraints and settlement rules remain intact.

Fleet-selected provider-outage recovery freezes `consultation_route_source:
fleet_configuration` together with its existing `fleet_provenance` in the same
canonical recovery document before native retirement. Retry and later generation
readback use that document's policy hash. A mere fleet match does not relabel a
credential-propagation or explicit profile recovery. Historical documents lacking
this marker remain profile-only; no marker or policy hash is backfilled.

### Fleet model catalog and route recognition

`/v1/catalog` projects the exact verified selected fleet snapshot. Each row is
keyed by a declared account and native model id and reports the symbolic model,
normalized provider family, vendor, efforts, vision, calibration and permission
eligibility with its policy hash. It invents no defaults, context limits,
pricing or account labels. Missing selection refuses as `fleet_catalog_unpinned`;
unactivated authoring does not replace the selected catalog.

Governed route validation uses only that snapshot's exact account/model/effort
membership. Known provider normalization is shared with the runtime, including
Cursor account suffixes; normalization alone grants no account or route. Unknown
providers refuse, duplicate native ids within a domain are invalid, and ambiguous
account/native-model matches across domains cannot resolve or appear in the
catalog. Fleet domain support remains Claude, Codex, Cursor and OpenCode.

Compiled-only bootstrap, pre-policy draft validation, historical Committee
successor selection and existing project Desk/PR consumers retain their compiled
catalog. Frozen recovery may use compiled compatibility only for the exact route
already persisted in its profile; fleet metadata comes from its immutable pinned
policy, never current policy. Neither mode widens governed selection. Core
`ModelRung::validate` still independently denies DeepSeek V4 Pro; moving that
remaining exclusion into policy is outside this capability checkpoint.
