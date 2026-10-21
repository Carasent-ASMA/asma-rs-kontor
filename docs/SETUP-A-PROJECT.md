# Set up a project with the Kontor workflow pack

> **Date:** 2026-10-21
> **Status:** Current — the setup runbook for the public workflow pack
> **Category:** runbook
> **Scope:** An empty repository to a first Paseo-direct epic, with ASMA as the
> worked example. The Kontor-governed option is described to its L3 boundary.
> **Summary:** The executable sequence in §0.1: layout, selected config,
> project records, render, adapter links, entry files, first plan, then the
> check and harness proof. The sections below are the reference. The selected
> config folder is explicit and recorded. JSON authoring sources migrate to
> YAML, and credentials stay references. The governed option is gated on its
> config-folder reader, which is not installed by this pack revision.

## When to Load

**Load this document when:**

- Setting up a new project from the Kontor workflow pack.
- Auditing a project's entry files, skills, router, config folder or harness
  load.
- Deciding whether a project is ready to leave Paseo-direct for a governed
  epic.

**Do NOT load for:** the method itself ([`WORKFLOW.md`](WORKFLOW.md)), the
word list ([`GLOSSARY.md`](GLOSSARY.md)), the principles
([`PRINCIPLES.md`](PRINCIPLES.md)), the configuration contract
([`CONFIGURATION.md`](CONFIGURATION.md)) or the decisions that govern the pack
([`decisions/`](decisions/README.md)).

## 0. Before you start

You need:

- a checkout of this repository at a pinned revision — it is both the pack and
  the public method;
- an empty or near-empty project repository with a tracker project and a forge
  repository;
- Python 3.11 or later, `git`, and the harnesses you will use (Claude Code,
  Codex, OpenCode);
- a secret store for the tracker and forge credentials. This runbook creates
  references only; it never handles the values.

Define two paths for the session. They are placeholders in this document and
must be absolute in the shell that runs the commands:

```sh
PACK=<absolute path to this asma-rs-kontor checkout>
PROJECT=<absolute path to the project checkout>
CONFIG="$PROJECT/config"
```

`CONFIG` may live outside the project checkout. Pick one folder and keep it for
every consumer and every machine service.

The runbook is a living by-name document, like `CONFIGURATION.md`; it is not
date-prefixed. Every other project document follows the naming rule in §2.

The finish line is:

1. `pack.py check` passes on the project.
2. Claude Code, Codex and OpenCode each load the project entry file, the
   project skills and the documentation router.
3. The plan for the first epic validates against the selected config folder.

The proof for this runbook is recorded in its pull request, not in this
document. A project repeats the same commands in its own checkout.

### 0.1 Executable order

The sections below are the reference. Execute them in this order:

1. **Create the layout.** Create the project tree and the selected config
   folder (§1). Keep both paths absolute for the session.
2. **Select and fill the config.** Copy the neutral folder (§7.1), fill every
   `project:` value (§7.3), and copy and fill `host.yml` (§6). Do not render
   before the `project:` block is filled.
3. **Write the project records.** Copy the method documents, then write the
   router, decision index, gap ledger and setup record; write the local
   untracked config-path record (§2, §7.2). Do not add a plan row yet.
4. **Render the skills.** Run `pack.py render` (§9).
5. **Create the adapter links.** Create the `.claude` skill links, including
   `.pack`, after render (§3). For a separate module root, run the adapter
   `--dry-run`, `--apply`, `--check` sequence (§3).
6. **Write the entry files.** Fill `AGENTS.md` from the selected config and
   write the one-line `CLAUDE.md` (§3).
7. **Create the first plan and its route.** Write the plan, add its router row
   in the same change, then sync and validate (§10). Do not route a plan that
   does not exist.
8. **Check and prove.** Run `pack.py check` (§9) and the harness checks (§15).
   Record the evidence; keep `.local/` and the logs untracked.

## 1. Layout (§8.1)

Create the project layout. The first block is the repository; the second is the
config folder, which may live outside the repository.

```text
<project>/
  AGENTS.md                       entry file, at most 8 KiB
  CLAUDE.md                       exactly one line: @AGENTS.md
  .github/skills/<skill>/         canonical rendered skills, plus .pack resources
  .agents/skills/<skill>/         byte-for-byte mirror for Codex and OpenCode
  .claude/skills/<skill>          symlinks into .github/skills for Claude Code
  .local/                         local setup records and harness logs; git-ignored
  docs/                           the documentation root for this example
    index.md                      the router: one row per document
    WORKFLOW.md
    GLOSSARY.md
    PRINCIPLES.md
    GAPS.md
    decisions/README.md
    plans/index.md
    plans/<plan>.md
    operations/<setup-record>.md
  config/                         the selected config folder (or an external path)
    .gitignore                    ignores host.yml
    README.md                     neutral folder notes
    attention.yml                 attention channel and message-class policy
    orchestration/
      orchestration.yml           entry: schema_version, fleet, core_team, project:
      orchestration.schema.yml    required-key schema for project:
      fleet.yml
      route-qualifications.yml
      teams/core-team.yml
      tpm-supervision.yml
      pr-desk.yml
    tracker/jira.yml
    packs/team.yml
    forge/github-app.example.yml
    host.example.yml
    host.yml                      this machine only; git-ignored
    quota-signals.yml
    paseo-supervision.yml         sample for <state-root>/supervision.yml
```

`cp -R "$PACK/config/examples/." "$CONFIG/"` copies this whole tree, including
`attention.yml`, `.gitignore` and `README.md`; do not build a hand-picked
subset. The single-repository example uses `docs` as its documentation root and
`config` as its config folder. A multi-repository project keeps one
`resource_roots` entry per repository and may keep the config folder in the
repository that owns the project, or outside every checkout. The root names are
parameters; nothing in the pack requires `docs` or `config`.

The public ASMA values in the `project:` block in
[`tests/fixtures/config/asma-orchestration.yml`](../tests/fixtures/config/asma-orchestration.yml)
are the schema's test case, not a live config folder. They show the shape a
concrete project fills in: `ASMA` as the tracker key, `Carasent-ASMA` as the
forge organization, `Carasent-ASMA/asma-modules` and
`Carasent-ASMA/asma-rs-kontor` as the repositories, `master` as the base branch
and `asma` as the project CLI. The ASMA setup record describes the state after
WP-07 and WP-08: the `project:` block is installed in ASMA's selected `config/`
folder, the documentation roots are `modules/_docs` and `kontor/docs`, and the
absolute path is kept in ASMA's local setup record and service environment,
never in this public repository. Until WP-07 and WP-08 land, do not point
`pack.py --config-dir` at ASMA's current
`config/orchestration/orchestration.yml`; it is a three-line selector with no
`project:` block, and the tool refuses it with `project is missing ...`.

## 2. Documentation governance (§8.2)

Every adopter uses the same rules. The pack's method lives in
[`WORKFLOW.md`](WORKFLOW.md); the words live in [`GLOSSARY.md`](GLOSSARY.md);
the principles live in [`PRINCIPLES.md`](PRINCIPLES.md).

1. **Three homes.** A decision is a reviewed record. A live fact is read live,
   or written "as of <time>, from <command>". History is a dated report. Never
   store a live fact in prose.
2. **Names and headers.** A dated document is
   `yyyy-mm-dd-hh-mm-<category>-<slug>.md`, with a header of Date, Status,
   Author (or role), Category, Scope, Summary, When to Load and Do NOT load for.
   Living by-name documents keep their name.
3. **Routed once.** Every document has exactly one row in its owning
   index/router. `pack.py check` refuses an unrouted document, a document
   routed twice and a root without a router.
4. **One status vocabulary and the evidence ladder.** A completion claim names
   the rung — implemented, merged, qualified, released, deployed with readback,
   enabled — and its evidence. A green graph is not a delivered epic.
5. **Plans** use the status vocabulary. A plan with `status: On Hold` carries a
   `hold:` block with `reason`, `owner`, `lifts_when` and `review_by`; the
   review date is at most 30 days ahead. `pack.py check` enforces the closed
   reason list and the date.
6. **Decision ids** are qualified by their document:
   `<file-slug>#DEC-<nn>`.
7. **One gap ledger.** Every gap report goes to the project's one ledger with
   the §6 fields: a stable id, a type (`kontord`, `workflow` or `tool`),
   severity, evidence, observed revision, owner, closure condition and status.
8. **Lifecycle.** Draft → approved → delivered → compacted into a dated summary
   in `history/`. Living documents are never archived.
9. **Publication class.** A public project carries only class P material. The
   pack's `public_tree.py` and `pack.py check` refuse home paths, run receipts
   and provider-account aliases; a project's own publication review still
   owns people, accounts, costs and private links.
10. **Decision records.** Product decisions go to the project's decision
    records. Kontor decisions go to this repository's
    [`decisions/`](decisions/README.md). Only public decisions go public.
11. **Memory.** Knowledge goes into documents. A harness's own memory keeps
    pointers and personal preferences only.

Copy the three method documents into the project's documentation root at the
pack revision, then write the project's own router and records:

```sh
mkdir -p "$PROJECT/docs/decisions" "$PROJECT/docs/plans" \
  "$PROJECT/docs/operations"
cp "$PACK/docs/WORKFLOW.md" "$PACK/docs/GLOSSARY.md" \
  "$PACK/docs/PRINCIPLES.md" "$PROJECT/docs/"
```

The project's `docs/index.md` is the router. Use
[`workflow/templates/index-router.md`](../workflow/templates/index-router.md)
as the shape. A minimal router is:

```markdown
# Project documentation index

| Document | Summary | Status | When to load |
| --- | --- | --- | --- |
| [Workflow](WORKFLOW.md) | The Kontor method on both surfaces | Current | Every delivery task |
| [Glossary](GLOSSARY.md) | The one dictionary | Current | When a term is unclear |
| [Principles](PRINCIPLES.md) | The governing principles | Current | When a rule needs its reason |
| [Plans](plans/index.md) | Active plans | Current | Before an epic |
| [Decisions](decisions/README.md) | Project decision records | Current | Before an architectural change |
| [Gap ledger](GAPS.md) | One gap ledger | Current | When recording a gap |
```

`plans/index.md` is the owning router for plan documents. It uses the same
table header. The gap ledger and decision index are project-owned files; the
pack's [`docs/decisions/README.md`](decisions/README.md) is the shape, not the
project's decisions.

The minimum project-owned files are:

```markdown
<!-- docs/GAPS.md -->
# Project gap ledger

| Id | Type | Severity | Gap | Evidence | Observed revision | Owner | Closes when | Status |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| G-001 | workflow | advisory | No gaps recorded yet. | The first project check. | <revision> | <owner> | A real gap is recorded. | open |
```

```markdown
<!-- docs/decisions/README.md -->
# Project decisions

Project product and architecture decisions live here. Kontor decisions stay in
the Kontor repository.

A decision id is qualified by its file: `<file-slug>#DEC-<nn>`. A decision is
accepted only by the authority that owns its scope.

| Id | Decision | Status | Authority | Supersedes |
| --- | --- | --- | --- | --- |
| — | No decisions yet | — | — | — |
```

```markdown
<!-- docs/plans/index.md -->
# Project plans documentation index

| Document | Summary | Status | When to load |
| --- | --- | --- | --- |
| — | No plans yet; add the plan and its row together | — | — |
```

Add a plan's row and the plan file in the same change. A router row for a file
that does not exist yet is a broken link, and `pack.py check` refuses it. The
placeholder row is replaced in §0.1 step 7, not during the initial record
setup.

A plan is a document too. Copy
[`workflow/templates/plan.md`](../workflow/templates/plan.md) and fill the
document placeholders; the `{{ document.* }}` values belong to the author, not
to the config folder. The `{{ project.* }}` values come from the selected
config folder in §7.

## 3. Instruction setup and what each harness reads (§8.3)

One entry file, one skill source, one router. The harness table is the contract:

| Harness | Entry | Skills |
| --- | --- | --- |
| Codex | `AGENTS.md` | `.agents/skills` |
| Claude Code | `CLAUDE.md` → `@AGENTS.md` | `.claude/skills` |
| OpenCode | `AGENTS.md` | `.agents` and `.claude`, deduplicated by name |
| Cursor | `AGENTS.md`, `CLAUDE.md`, `.cursor/rules` | symlinks into `.github` |
| Copilot | `copilot-instructions.md` plus `applyTo` files | not portable |

Write the entry file from
[`workflow/templates/AGENTS.md`](../workflow/templates/AGENTS.md). The template
has a Workflow part and a Project part. Replace every `{{ project.* }}` with the
value in the selected `orchestration.yml`; the Project facts table is the
mapping. Keep the result at most 8 KiB:

```sh
cp "$PACK/workflow/templates/AGENTS.md" "$PROJECT/AGENTS.md"
# Edit AGENTS.md; then:
test "$(wc -c < "$PROJECT/AGENTS.md")" -le 8192
! grep -n '{{' "$PROJECT/AGENTS.md"
```

`CLAUDE.md` is exactly one import line. It must not route back to itself:

```sh
printf '@AGENTS.md\n' > "$PROJECT/CLAUDE.md"
```

OpenCode reads `AGENTS.md` without a config file. If the project already has
`opencode.json`, its `instructions` list names the entry file only — not
`CLAUDE.md` or `copilot-instructions.md` — and the unconditional bytes stay at
most 16 KiB. The pack ships no `opencode.json` generator in this revision. Do
not hand-maintain a second editable source: treat a state-root or native JSON
file as a generated projection with a recorded YAML source and content hash,
and fence that projection until WP-08 owns its generator. The proof leaves it
absent.

The **router** in this runbook is the documentation router `docs/index.md`. The
harnesses read it on demand through the file tools; the entry file and the
skills are loaded automatically. A task/path procedure router is optional and
project-owned: if you add `.github/agent-policy-routing.md`, it points at the
owning skill and is linked from `AGENTS.md`. The pack does not ship a template
for that optional file (gap G-04).

The skills have one source and one mirror. The canonical copy is
`.github/skills`; `pack.py render` writes it and the `.agents` byte mirror. The
`.claude` catalog is a symlink view of the canonical copy. Create the links
after render:

```sh
mkdir -p "$PROJECT/.claude/skills"
for d in "$PROJECT"/.github/skills/*/; do
  name=$(basename "$d")
  ln -sfn "../../.github/skills/$name" "$PROJECT/.claude/skills/$name"
done
ln -sfn "../../.github/skills/.pack" "$PROJECT/.claude/skills/.pack"
```

The `.pack` link is separate because the shell `*` glob does not match a
leading dot. Without it, a rendered skill's `../.pack/...` link works in the
canonical `.github` tree but not through the `.claude` symlink view.

`pack.py install-adapters` is for a separate module or worktree root where both
`.agents` and `.claude` are adapters into this checkout:

```sh
MODULE_ROOT=<absolute module or worktree root>
python3 -B "$PACK/scripts/pack.py" install-adapters "$PROJECT" \
  --target-root "$MODULE_ROOT" --dry-run
python3 -B "$PACK/scripts/pack.py" install-adapters "$PROJECT" \
  --target-root "$MODULE_ROOT" --apply
python3 -B "$PACK/scripts/pack.py" install-adapters "$PROJECT" \
  --target-root "$MODULE_ROOT" --check
```

On a fresh root run `--dry-run`, then `--apply`, then `--check`. `--check`
before `--apply` exits 2 because the links do not exist yet; it is the
post-install verification. It is not run against the project root after
`render`, because render already owns `.agents/skills` as the byte mirror.

The adapter scan skips the `.pack` dot-directory, so add the same `.pack` link
beside both adapter catalogs after `--apply`; each link points at the project's
canonical resources:

```sh
for catalog in "$MODULE_ROOT/.agents/skills" "$MODULE_ROOT/.claude/skills"; do
  ln -sfn "$(python3 -c 'import os,sys; print(os.path.relpath(sys.argv[1], sys.argv[2]))' \
    "$PROJECT/.github/skills/.pack" "$catalog")" "$catalog/.pack"
done
```

This is part of the workaround in gap G-02, not a separate link source.

## 4. Roles and personas (§8.4)

Role codes are canonical. A persona is a label on a role slot, never a different
authority. The method's role table is in
[`WORKFLOW.md`](WORKFLOW.md#2-roles-and-personas); the full cards and the
crosswalk (role code ↔ fleet slot ↔ seed role ↔ seat label) are in
[`workflow/skills/seat-cards/SKILL.md`](../workflow/skills/seat-cards/SKILL.md).

Use only the roles the project's teams use. Every card has the same shape:
purpose, inputs and outputs, tool profile, stop conditions and one personality
line.

| Role | Owns |
| --- | --- |
| `LSA` — Lead Software Architect | Architecture, scope coherence, cross-task decisions and remediation design |
| `TPM` — Technical Program Manager | Routing, admission, dependencies, evidence, closeout and the plan revision |
| Adam | The coordinator persona that fills the TPM slot for one epic; the single human-facing tab |
| `SWE` — delivery seat | One scoped task in its own checkout and the task evidence |
| `QA` — Quality Assurance Engineer | Independent task-level verification; a rejection returns rework to the fix owner |
| `AUD` — Auditor | Independent audit beyond ordinary tests |
| `REV A`, `REV B`, `JUDGE` | Read-only reviewers and the fresh finding checker |
| `REV SEC`, `REV DB` | Specialist security and database reviewers |

The independence rules are not optional:

- the two reviewers run on contrasting providers;
- each seat's model comes from its fleet slot binding, and the report records
  it;
- the judge is fresh and reads both findings only after both are complete;
- committee seats are read-only, and their tool tier enforces it;
- nobody reviews their own work.

The coordinator and the advisor are separate identities. Start the premium
advisor only on the fixed trigger: an architecture or decision-record question,
a scope change, a blocking reviewer disagreement, a second rejection, security,
tenant data, money, or the coordinator being unsure. Advice is evidence, never
an authority grant.

Team shape follows risk, not preference:

| Tier | When | Team |
| --- | --- | --- |
| Chore | Docs, rename, config bump, lint, scaffold, low-risk research | Smallest profile |
| Standard | Ordinary feature, bug, refactor or test in one module | Coordinator, delivery seat(s), two independent reviewers and a judge |
| High-stakes | Auth, permissions, tenant isolation, migrations, money, security, clinical safety, destructive operations, cross-service contracts, ADR-level choices | High-stakes profile, independent review, all gates, advisor/QA/audit as the plan requires |

Unclear cases count as high-stakes. Capacity starts at the value in
`project.capacity.start_admission`, rises one at a time after clean checks, and
never exceeds `project.capacity.max_parallel_tasks` for placement. A
`workflow: free` epic follows only the safety floor and the pieces it names.

## 5. Skills per role (§8.5)

The pack renders the workflow skills; the project adds its domain, module and
code-discovery skills beside them. The catalog is
[`workflow/skills/README.md`](../workflow/skills/README.md).

| Role | Pack skills |
| --- | --- |
| Coordinator (Adam / TPM) | `epic-orchestration`, `mini-project-kickoff`, `agent-dispatch`, `seat-handoff`, `seat-takeover`, `ticket-triage-tiers`, `long-task-commit-checkpoints`, `tracker-linked-creation`, `kontor-communication` |
| Advisor (LSA) | `decision-records`, `create-implementation-plan`, `document-organization` |
| Delivery (SWE) | `long-task-commit-checkpoints`, the project's code-discovery and module skills |
| Reviewers and judge | `pr-review`, `seat-cards`, `committee-protocol` |
| Everyone | `docs-navigation`, `experience-memory`, `ubiquitous-language`; `mutation-testing` when the plan requires it |

Each repeated rule has one owner. Link to the owning skill instead of copying
its text. The pack's `workflow/skills/README.md` holds the ownership table and
`pack.py check` checks the rendered placeholders and metadata.

## 6. Teams and fleet (§8.6)

One authoring source for both surfaces: the selected folder's
`orchestration/fleet.yml`. `host.yml` holds this machine's working routes,
paths, binaries, ports, account aliases and credential references. The fleet
holds the canonical model ids, vendors, efforts, chains and seat bindings; the
host holds the working provider/harness route. The join key is the logical
account slot.

The files have different jobs:

| File | Owns |
| --- | --- |
| `orchestration/fleet.yml` | Domains, account slots, canonical models, chains, seat bindings, `independent_of` |
| `orchestration/teams/core-team.yml` | The pinned Core Team roster and its mandatory seats |
| `packs/team.yml` | The project's team/profile overlay, committee presets and role prompts |
| `orchestration/tpm-supervision.yml` | The autonomous TPM cadence profile; `project.supervision` names it |
| `orchestration/pr-desk.yml` | Desk start budget, active-PR policy, silence windows and round limits; `project.pr_desk.settings` names it |
| `host.example.yml` | Committed host schema with placeholder values |
| `host.yml` | This machine only, git-ignored: absolute paths, binaries, endpoints, routes and credential references |
| `orchestration/route-qualifications.yml` | Hash-bound route qualification records; an activation record binds the exact bytes |

The neutral example in [`config/examples/`](../config/examples/README.md) is a
complete placeholder folder. Copy it; replace every account, model id, Core
Team identity and route with the project's own. Keep the fallback chains per
role, keep the premium "hard decision" chain off the default route, and keep
vendor independence for reviewers. Accounts, costs, provider aliases and
credential values are class X; they never enter this public repository.

The fleet is activated policy: preview, publish and activate; an active run
keeps its resolved-route snapshot. `host.yml` is bootstrap input, never
preview/publish/activated policy: it is read before the daemon starts, a change
requires a restart for new placements, and it never enters the activation
path. A host route change never requires a fleet edit. `project.runtime.matrix`
is the allowlist of provider/harness pairs; a route outside it is refused
before placement. See [`CONFIGURATION.md` §Bootstrap and activated
settings](CONFIGURATION.md#bootstrap-and-activated-settings).

## 7. Parameters, selection and migration (§8.7)

### 7.1 Select the folder explicitly

Select exactly one config folder for the project/user installation. Start
from the neutral example, then replace its placeholders:

```sh
mkdir -p "$CONFIG"
cp -R "$PACK/config/examples/." "$CONFIG/"
```

The flag wins over the environment variable:

```sh
# Explicit flag; a relative input resolves once against this command's cwd.
python3 -B "$PACK/scripts/pack.py" --config-dir "$CONFIG" render "$PROJECT"

# The same selection through the environment.
KONTOR_CONFIG_DIR="$CONFIG" python3 -B "$PACK/scripts/pack.py" check "$PROJECT"
```

The rules are:

- `--config-dir` wins over `KONTOR_CONFIG_DIR`.
- A relative input is resolved once against the setup invocation's working
  directory, then the absolute selection is persisted for later commands and
  service launches.
- There is no fallback to the current directory, a state root, an ASMA path or
  an old file. A missing selection, missing folder, missing referenced file or
  invalid YAML is refused by name.
- The same selected YAML source serves Paseo-direct and Kontor-governed work.
  Setting up the direct option does not require `kontord`.
- Relative config references (`fleet`, `core_team`, `project.team_pack`,
  `project.fleet`, `project.supervision`, `project.tracker.spec`,
  `project.pr_desk.settings`) resolve from the selected folder root.
- Project resource paths (`documentation_roots`, `gap_ledger`, `glossary`,
  `decision_records_root`) begin with a `project.resource_roots` key and
  resolve inside that repository checkout, never from the config folder.
- A project with more than one `resource_roots` key passes one
  `--resource-root KEY=PATH` per key to `pack.py check`; a declared root
  without an override is reported as unchecked, and a required documentation
  path under it fails. The ASMA example declares `modules` and `kontor`.
- `host.paths.*`, `host.binaries.*` and `host.project_cli.*` are absolute or
  PATH-resolved host state.

### 7.2 Record the resolved path

`render` and `check` print the resolved folder. The exact absolute path is host
data. Keep it in a local, untracked record outside every documentation root;
the routed setup record carries only the portable selection and a reference to
that local record.

```sh
LOCAL_RECORD="$PROJECT/.local/setup/config-folder"
mkdir -p "$(dirname "$LOCAL_RECORD")"
printf '%s\n' "$CONFIG" > "$LOCAL_RECORD"
GITIGNORE="$PROJECT/.gitignore"
if ! grep -qxF '.local/' "$GITIGNORE" 2>/dev/null; then
  if [ -s "$GITIGNORE" ] && [ -n "$(tail -c 1 "$GITIGNORE")" ]; then
    printf '\n' >> "$GITIGNORE"
  fi
  printf '.local/\n' >> "$GITIGNORE"
fi
git -C "$PROJECT" check-ignore -v .local/setup/config-folder
```

The append starts on a new line even when an existing `.gitignore` has no
final newline; `git check-ignore` is the proof that the record is ignored.

The check prints the resolved path in its final line, for example:

```text
pack check passed: <project> (config <absolute folder>)
```

Write that exact folder to `$LOCAL_RECORD` and to the service environment, never
to a routed document, a skill, a template or a submitted command. The public
lint refuses a home path in a routed document; `.local/` is outside the
documentation roots and git-ignored. A minimum local record and routed record
are:

```text
# .local/setup/config-folder (git-ignored; never routed)
<absolute path printed by pack.py>
```

```markdown
---
status: Current
orchestration: paseo
workflow: kontor
---

# Project setup record

> **Date:** <date and time> <time zone>
> **Category:** record
> **Scope:** This project's pack pin and config-folder selection.
> **Summary:** The pack revision, the portable selection and the location of
> the local absolute-path record.

## Pack source

| Fact | Value |
| --- | --- |
| Pack repository | <organization>/<repository> |
| Pack revision | <git revision> |
| Setup date | <date> |

## Config folder

| Fact | Value |
| --- | --- |
| Selection | `--config-dir config` on setup; `KONTOR_CONFIG_DIR` for later commands |
| Input | `config` (relative to the project checkout) |
| Exact path location | Local, untracked: `.local/setup/config-folder` (outside the documentation roots) |
| Entry file | `orchestration/orchestration.yml` |
| Entry file SHA-256 | <sha256> |
```

The routed record never contains the absolute path. `pack.py check` scans the
routed tree, so a host path there would fail the `home_path` lint.


### 7.3 Fill the `project:` block

`orchestration/orchestration.yml` is the folder's entry file. It carries
`schema_version`, the `fleet` and `core_team` selectors, and the `project:`
block. Every relative reference must stay inside the folder and exist; every
required key is in
[`orchestration.schema.yml`](../config/examples/orchestration/orchestration.schema.yml).

| Parameter group | Keys | ASMA example (public) | Neutral example |
| --- | --- | --- | --- |
| Identity | `key`, `name`, `owner` | `ASMA`, `ASMA`, `platform owner` | `EXM`, `Example Project`, `Platform Owner` |
| Resources | `resource_roots`, `decision_authorities` | `modules: Carasent-ASMA/asma-modules`, `kontor: Carasent-ASMA/asma-rs-kontor`; decisions in `modules/_docs/adr/` | `project: example-org/example-project`; decisions in `project/docs/decisions/` |
| Tracker | `tracker.kind`, `site`, `project_key`, `key_pattern`, `credential_alias`, `spec` | Jira, `ASMA-<n>`, `tracker/jira.yml` | Jira, `EXM-<n>`, `tracker/jira.yml` |
| Branch and commit | `branch.types`, `pattern`, `commit_pattern` | `<type>/ASMA-<number>-<slug>`, `^ASMA-<number> <summary>$` | `<type>/<KEY>-<number>-<slug>` |
| Forge | `forge.kind`, `organization`, `repositories`, `base_branch` | GitHub, `Carasent-ASMA`, both repositories, `master` | GitHub, `example-org`, one repository, `main` |
| Runtime and CLI | `runtime.matrix`, `cli.command`, `cli.verbs` | Paseo harness pairs; `asma` | Paseo/OpenCode pairs; `example` |
| Team and fleet | `team_pack`, `fleet`, `supervision` | `packs/team.yml`, `orchestration/fleet.yml`, `orchestration/tpm-supervision.yml` | Same paths |
| Desk and merge | `pr_desk.command`, `settings`, `history_root`; `merge_authority.role`, `policy` | The project desk command; independent review then owner | `example-pr-desk`; `independent-review-then-owner` |
| Documentation | `documentation_roots`, `gap_ledger`, `attention_channel`, `glossary` | `modules/_docs`, `kontor/docs`; `modules/_docs/ai-orchestration/index.md`; `desk:adam`; `modules/UBIQUITOUS_LANGUAGE.md` | `project/docs`; `project/docs/GAPS.md`; `attention:platform-owner`; `project/docs/GLOSSARY.md` |
| Checkout and checks | `worktree_layout`, `check_runs` | `.worktrees/<key-slug>/<module>`; the project's check names | `.worktrees/<key-slug>/<module>`; `publication-branch-title` |
| Capacity | `capacity.max_delivery_seats`, `max_parallel_tasks`, `start_admission` | `2`, `7`, `4` | `1`, `2`, `1` |
| Calendar | `time_zone`, `holiday_countries`, `currency` | `Europe/Oslo`, `['NO']`, `NOK` | `Europe/Oslo`, `['NO']`, `NOK` |

The table shows the required quoted form. The shipped neutral example still has
`holiday_countries: [NO]`; YAML 1.1 reads that as `[false]`, so a render shows
`Holidays | false` (gap G-03). Quote two-letter country codes as `['NO']`.

The same YAML 1.1 trap applies to `yes`, `no`, `on` and `off`: quote them as
`'yes'`, `'no'`, `'on'` and `'off'`. Quote numeric-looking versions such as
`'1.10'`; an unquoted `1.10` is a number, not the string the consumer expects.
Parse the resolved value, not the source spelling.

`orchestration.yml` is not a second copy of the fleet or the team pack. It
selects them. `host.yml` is the machine's file; copy `host.example.yml`, replace
every path, binary, endpoint, route and alias, and keep it git-ignored. It never
contains a credential value.

### 7.4 JSON-to-YAML migration and credential references

Authored configuration is YAML only. Where a runtime or external tool requires
JSON or another native format, that file is a generated projection with a
recorded YAML source and content hash; it is never a second editable source.
The full inventory and migration table are in
[`CONFIGURATION.md`](CONFIGURATION.md#configuration-authoring-inventory-wp-02-refresh).

The migration procedure:

1. **Inventory** every human-edited JSON source and its readers. A generated
   contract, a test fixture, evidence, a tool-native manifest and a database
   are not authoring configuration.
2. **Write the YAML replacement** in the selected folder. Host-only values go
   to `host.yml`; project values go to `orchestration/`, `tracker/` or
   `packs/`.
3. **Back up** the old file and read it once.
4. **Compare the resolved values**, not the syntax. The migration is not a
   reformat; every reader must resolve the same project, tracker, fleet, team,
   route, path and credential reference.
5. **Update every reader and check** in the same change. A reader that still
   reads the old path keeps the old source alive.
6. **Remove the old editable path** after the comparison and the reader update.
   Refuse both old and new sources together with a migration instruction.
7. **Generate a native projection only when a consumer still needs it**, with
   the source path and the content hash recorded. Keep the projection out of
   the authored set; regenerate it instead of editing it. State-root JSON files
   that a runtime writes are projections, not authored sources; the pack ships
   no generator for them in this revision, so fence them until WP-08 owns the
   generator and its recorded source and hash. Do not create or edit them by
   hand.
8. **Preserve active pins and credential references.** A policy edit has no
   live effect until it is previewed, published and activated; a running work
   item keeps its pin.

Credential values never enter YAML, logs, exports, command arguments, fixtures,
tickets or documents. YAML names a reference; the secret store holds the value:

```yaml
tracker:
  credential_alias: <tracker-credential-alias>
```

```yaml
provider_accounts:
  <logical-slot>:
    credential: keychain:<service>/<account>
```

The reference names a secret-store entry. It is not an email address, a token,
a provider home or a key path inside a repository. The PR desk, tracker and
runtime resolve the value at use time from the deployment's secret store.

The pack does not ship a project CLI, a PR desk or a credential resolver.
`project.cli.command` and `project.pr_desk.command` name the adopter's own
commands; `credential_alias` and `credential` name entries in the adopter's
secret store. A project without a desk keeps the placeholder command and runs
the review manually, and records the missing desk in its own gap ledger. A
project without a secret store does not put values in YAML: it resolves the
reference through its own store or records the gap and stops the tracker step.

## 8. What the project owns (§8.8)

| Ships in the pack (public) | The project owns |
| --- | --- |
| The workflow guide, glossary, principles and Kontor decisions | Its product and architecture decisions |
| The workflow skills, seat cards and templates | Its domain, module and code-discovery skills |
| The neutral config example, the `project:` schema and `host.example.yml` | Its `project:` values, tracker key, fleet values, accounts and routes |
| The pack tool's render, check, install-adapters, sync and validate commands | Its entry Project part, personas and role prompts |
| The public-example lint and the method documents | Its merge grants, people, authority and costs |
| The neutral seeds and committee template | Its team/profile overlay, committee presets and tracker specs |

Copy the pack's three method documents into the project's documentation root at
the pinned revision. Copy the templates, then make them yours. A rendered skill
under `.github/skills` is generated: edit the pack source and re-render, never
the rendered copy. Record the pack revision in the setup record; on an upgrade,
re-render, re-copy the method documents and templates, and re-run the checks.

ASMA's split is the worked example: the pack owns the method and the neutral
examples; ASMA owns its `AGENTS.md` Project part, its `config/` values, its
tracker key, fleet accounts and routes, merge grants, decision records, domain
skills, persona overlays, costs and clinical role packs. Those values are not
copied into this repository.

## 9. Render, adapt and check

From the project checkout:

```sh
python3 -B "$PACK/scripts/pack.py" --config-dir "$CONFIG" render "$PROJECT"
```

`render` writes the canonical skills, the `.pack` resources and the `.agents`
byte mirror. It refuses an unfilled placeholder instead of shipping `{{ ... }}`.
Then create the `.claude` links from §3 and write the entry files from §3.

Run the check:

```sh
python3 -B "$PACK/scripts/pack.py" --config-dir "$CONFIG" check "$PROJECT"
```

A multi-repository project supplies one `--resource-root` per declared
`project.resource_roots` key. The ASMA example declares `modules` and `kontor`,
so its check names both checkouts (absolute placeholders here; the real paths
are host data):

```sh
python3 -B "$PACK/scripts/pack.py" --config-dir "$CONFIG" check "$PROJECT" \
  --resource-root modules=<absolute asma-modules checkout> \
  --resource-root kontor=<absolute asma-rs-kontor checkout>
```

A declared root without an override is reported as unchecked; a required
documentation path under it fails. A single-root project maps its one root to
the project checkout automatically.

Run the check once the entry files, the router and every routed document exist,
and again after §10 creates the first plan. It checks:

- every pack placeholder resolves from the selected folder and no rendered
  placeholder remains;
- the `.agents` mirror is byte-identical, metadata is present and the Claude
  catalog points at the canonical source;
- the entry budgets, the `CLAUDE.md` import and the OpenCode instruction chain;
- links and anchors under the documentation roots and the skill tree;
- one router per documentation root, every document routed exactly once;
- `hold:` blocks and their review dates;
- plan graphs and the derived view;
- the public-example lint.

A failure names the file and the rule. Fix the source, re-render when a skill
changed, and do not disable a check to get a pass.

## 10. The first Paseo-direct epic

The default surface is Paseo-direct. It needs no daemon and no `kontord`.

1. **Write the plan.** Copy
   [`workflow/templates/plan.md`](../workflow/templates/plan.md), fill the
   document placeholders, set `orchestration: paseo`, and record the tracker
   epic and task bindings. The plan is the durable authority for scope,
   dependencies, waves, gates and closeout.
2. **Write the task table and the graph.** The table has one row per local task
   id and a non-empty closure condition. The `plan-graph` block in §2.5 is the
   only editable dependency source; the derived view is generated.
3. **Sync and validate** with the same selected folder:

   ```sh
   python3 -B "$PACK/scripts/pack.py" --config-dir "$CONFIG" sync \
     "$PROJECT/docs/plans/<plan>.md"
   python3 -B "$PACK/scripts/pack.py" --config-dir "$CONFIG" validate \
     "$PROJECT/docs/plans/<plan>.md"
   ```

4. **Create the tracker items** through the project CLI and the
   `tracker-linked-creation` skill. The epic description has Summary / purpose,
   Goal, Scope and sub-goals, Done means and Detailed plan, in that order. A
   task has Summary / purpose, Goal, Scope and sub-goals, Delivery context,
   Verification and evidence, Detailed plan and Done means. The testable
   closure sentence is copied verbatim from the plan.
5. **Run readiness and admission.** Read the entry file, the plan and the
   config folder. Reconcile the plan, the tracker bindings and source control.
   Confirm one tracker epic binding and one issue binding per delivery task.
   Validate the graph; a failed validation blocks readiness. Check the epic
   description and the capacity. A hold at kickoff carries the same four
   fields.
6. **Deliver.** A delivery seat works in its own checkout, stages only its
   paths, runs the project's checks and hands off the exact revision with its
   evidence. A review is independent and read-only; nobody reviews their own
   work.
7. **Close out.** Merge only through the project's merge authority. State the
   evidence rung. A merged pull request is not a released, deployed or enabled
   claim. Record any gap in the gap ledger with an owner and a closure
   condition. A green graph is not a delivered epic.

The proof for this runbook stops before a real tracker write: it uses synthetic
task keys and a disposable repository, so no external tracker, forge or
credential is touched.

## 11. Validate both plans against the same YAML source

The same selected folder is the source for both orchestration surfaces. This
section is a `pack.py` validation, not proof that the daemon reads the folder.
The pack validates both plan markers against that one folder:

```sh
python3 -B "$PACK/scripts/pack.py" --config-dir "$CONFIG" validate \
  "$PROJECT/docs/plans/<paseo-plan>.md" \
  "$PROJECT/docs/plans/<kontor-plan>.md"
```

Each `PASS` line proves the plan's key pattern, epic binding and task bindings
resolved from the same `project:` block. `pack.py check` prints the resolved
folder in its final line, so the local record and the check name the same path.
The flag, the environment variable and an invocation from another working
directory **with the absolute path** must print the same absolute folder; the
proof in the pull request shows all three.

The governed plan that cannot launch yet is `status: On Hold`, not `Draft`. A
`Draft` marker claims the plan is waiting for approval; this plan is waiting for
a dependency. Carry the four hold fields:

```yaml
status: On Hold
hold:
  reason: waiting_on_dependency
  owner: <owner>
  lifts_when: WP-08 config-folder reader is released, deployed with readback and the L3 entry conditions are met.
  review_by: <ISO date at most 30 days ahead>
```

`pack.py validate` checks the plan graph; `pack.py check` checks the hold's
closed reason, owner, lift condition and review date. A valid hold is not
launch authority.

What this does **not** prove is the daemon reader. `kontord` does not yet read
the selected folder in this pack revision; that is WP-08's config-folder
change. The governed plan is authored and validated now and becomes launchable
only at L3. Do not start `kontord` to fill that gap (G-01).

## 12. Add `kontord` later, by level (§9)

Autonomy grows in levels. Each level keeps everything below it and produces the
evidence the next level needs. The levels are capabilities, not a ladder every
project must climb: L2 on Paseo-direct is a permanent, valid place to stop.

| Level | You are here when | What it adds |
| --- | --- | --- |
| **L0 Manual** | One agent, a human drives each step | The entry file and the skills as context |
| **L1 Workflow on Paseo-direct** | Plans follow the contract; tracker keys, naming, checkout discipline, independent review and committees are used | The full method, checked by procedure and evidence |
| **L2 Checked workflow** | The pack checks run before each launch; holds carry four fields; one gap ledger; statuses follow the evidence ladder | `pack.py check` and the project's checks in the launch path |
| **L3 One governed epic** | `kontord` is deployed with readback, least-privilege tiers, correct seat profiles and checked adapters; one governed pilot closes without a new gap | Deterministic admission, evidence and tracker projection |
| **L4 Kontor by default** | The retirement condition is met and the operator makes a dated switch | Governed orchestration as the default |
| **L5 Self-improving** | Scorecard, gap collector, skill evals, post-release watch and a bounded learner | A measured learning loop |

The short path for a new project:

1. Copy the pack and fill the `project:` parameters.
2. Write the entry files and router; render, adapt and check.
3. Run one epic on Paseo-direct. Many projects can stop at L2.
4. Optional: install `kontord`, select the same config folder, import the Team
   Definition and fleet, then qualify and deploy with a receipt.
5. Run one governed pilot and repair each gap with the repair rule.
6. Switch the default by a dated decision.
7. Turn on the learning loop.

The L3 entry conditions are evidence, not a checklist of intentions:

- the `kontord` release is deployed and read back;
- the config-folder reader accepts `--config-dir` / `KONTOR_CONFIG_DIR`, and a
  missing folder, an invalid file and an unactivated edit each behave as §7
  says;
- the Team Definition, tracker specs, fleet, Core Team roster and team/profile
  packs are installed in the project's own realm;
- the adapters pass their check before a seat starts;
- least-privilege tiers are read back: no admin tier outside the coordinator,
  and a denied-tool test fails as expected;
- one small governed pilot closes with no new gap.

Until WP-08 lands, the executable setup proof stops at §11. The direct option
is complete; the governed option is authored but not launchable.

## 13. Known gaps and stop points

This section is the pack's one gap ledger. A gap in the pack, this runbook or
its proof is recorded here once, using the WORKFLOW §6 fields. An adopting
project keeps its own one ledger with the same fields; it links here for pack
gaps and does not copy these rows.

Severity is `blocking`, `major`, `minor` or `advisory`; only `blocking` stops
the affected step. Observed revision is the pack revision where the gap was
reproduced; the row carries forward until it closes. Status is `open`,
`carried` (the owner accepted it with evidence) or `closed` at a named rung.

| Id | Type | Severity | Gap | Evidence | Observed revision | Owner | Closes when | Status |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| G-01 | `kontord` | blocking | `kontord` does not read the selected config folder (`--config-dir` / `KONTOR_CONFIG_DIR`); the `project:` block is a schema fixture for the daemon until WP-08. | `rg -n 'config_dir\|--config-dir\|KONTOR_CONFIG_DIR' --glob '*.rs'` has no matches in this revision; `pack.py` is the only config-folder reader. | `fd792117` | WP-08 | `kontord` reads the selected folder, refuses a missing folder, an invalid file and an unactivated edit, and a governed plan validates against it. | open |
| G-02 | `tool` | minor | `pack.py install-adapters` refuses a project root after `render` because `render` owns `.agents/skills` as the byte mirror; its adapter scan needs `SKILL.md`, so it also skips the `.pack` dot-directory. | `install-adapters --check` on a rendered project root exits 2 with `adapter path is owned by the target`; the manual project and module `.pack` links are the workaround. | `fd792117` | WP-09 (fresh-project proof) | The project-root adapter path and the project and module `.pack` links work without manual repair, or WP-09 closes or carries it with fresh-project evidence. | open |
| G-03 | `workflow` | minor | The neutral `holiday_countries: [NO]` is YAML 1.1 `false`, so an entry file renders the holiday value as `false`. | `python3 -c "import sys; sys.path.insert(0, 'scripts'); import pack; print(pack.load_yaml_text('holiday_countries: [NO]'))"` prints `{'holiday_countries': [False]}`; a render shows `Holidays \| false`. | `fd792117` | WP-09 (fresh-project proof) | The neutral example quotes country codes and a fresh render shows `NO`, or WP-09 closes or carries it with fresh-project evidence. | open |
| G-04 | `workflow` | minor | The pack ships the documentation router but no task/path procedure-router template. | `workflow/templates/` has `index-router.md` and no task/path procedure-router template. | `fd792117` | WP-09 (fresh-project proof) | The pack ships the task/path router template, or WP-09 closes or carries it with fresh-project evidence. | open |
| G-05 | `tool` | advisory | `pack.py check` validates the project block, paths, skills, links, routers, holds and graphs; it does not validate fleet-route semantics, tracker field maps or `host.yml` routes. | `pack.py check` has no fleet-route, tracker-field-map or `host.yml` route validator. | `fd792117` | WP-08 (activation) / WP-10 (direct desk and CLI readers) | The activation path and the direct readers validate those files at their named rung, or WP-08/WP-10 close or carry the scope with evidence. | open |

WP-09 owns G-02, G-03 and G-04 through its fresh-project proof: that proof must
either close each row or mark it `carried` with its evidence. G-01 remains the
L3 stop. A gap is not closed by silence; a new gap gets a row here, not a
second ledger.

## 14. Done checklist

The setup is complete when all of these are true:

- [ ] The selected config folder is absolute, recorded in the local untracked
      record (`.local/setup/config-folder`), and the same path is printed by
      the flag and environment invocations.
- [ ] `.local/` is git-ignored, `git check-ignore -v
      .local/setup/config-folder` exits 0 and names `.gitignore`, and the
      routed setup record contains no absolute path.
- [ ] `pack.py check` passes and prints the resolved folder.
- [ ] `AGENTS.md` is at most 8 KiB with no `{{ ... }}`; `CLAUDE.md` is exactly
      `@AGENTS.md`.
- [ ] `.github/skills` is the canonical rendered tree; `.agents/skills` is its
      byte mirror; `.claude/skills` points at the canonical tree, including
      `.pack`.
- [ ] `docs/index.md` routes every document once, and each plan validates.
- [ ] `config/host.yml` exists for this machine and is ignored by version
      control.
- [ ] Claude Code, Codex and OpenCode pass the repeatable checks in §15: the
      entry row, the project skill catalog and the router row.
- [ ] The first Paseo-direct epic has a plan, tracker bindings, an independent
      review and a named evidence rung.
- [ ] The gap ledger has no silent item; G-01 remains the L3 stop.

## 15. Harness checks (repeatable)

Run these from the project checkout after §9. Keep every log under
`.local/setup/`; the logs contain host paths and are git-ignored. The expected
skill count is the number of rendered skills (19 in the neutral example):

```sh
SKILLS=$(find .github/skills -mindepth 1 -maxdepth 1 -type d ! -name .pack | wc -l)
```

### Claude Code

```sh
mkdir -p .local/setup
claude -p --allowedTools Read --output-format text \
  --debug-file .local/setup/claude-debug.log "Reply with OK."
grep -E "Loaded [0-9]+ unique skills" .local/setup/claude-debug.log
claude -p --allowedTools Read --output-format text \
  "Reply with exactly two lines and nothing else. Line 1: the 'Project' row of the 'Project facts' table in your project instructions. Line 2: the markdown table row in docs/index.md that starts with '| [Workflow]'."
```

Expected: the debug line reports `project: $SKILLS` (for example `project: 19`),
and the answer is the Project row and the exact Workflow router row.

### Codex

```sh
codex debug prompt-input "Report the first documentation router row." \
  > .local/setup/codex-prompt-input.json
rg -o 'r[0-9]+` = `[^`]*\.agents/skills`|\| Project \| [^|]*\|' \
  .local/setup/codex-prompt-input.json \
  | rg -F -e "$PROJECT/" -e '| Project |'
codex exec --sandbox read-only -C "$PROJECT" \
  "Reply with exactly two lines and nothing else. Line 1: the 'Project' row of the 'Project facts' table in your project instructions. Line 2: use your shell tool to find the line starting with '| [Workflow]' in docs/index.md and reply with that line."
```

Expected: the prompt-input output names the project `.agents/skills` root and
the Project row, and the run prints both rows.

### OpenCode

```sh
opencode debug skill > .local/setup/opencode-skills.json
rg -F "$PROJECT/" .local/setup/opencode-skills.json | wc -l
opencode run --pure -m <provider/model> \
  "Reply with exactly two lines and nothing else. Line 1: the 'Project' row of the 'Project facts' table in your project instructions. Line 2: use your file-read tool to read docs/index.md and reply with the table row that starts with '| [Workflow]'. Copy the row exactly; do not paraphrase."
```

Expected: the count is `$SKILLS`, and the run prints both rows. Choose a
configured model from `opencode models`; the model id is host data, not a
project value.

A paraphrase is not a pass. The router row must match `docs/index.md`, and the
skill count must match the rendered tree. Keep the harness logs local; do not
commit them.
