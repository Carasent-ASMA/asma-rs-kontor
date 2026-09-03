# KON-OP-07 architecture handoff

Date: 2026-08-17
Status: approved for implementation
Scope: native Jira ownership, ASMA Epic activation, project/subject authority cutover and legacy Jira-write refusal

## Decision

Make two changes behind the OP-03 `ApplicationOperations` boundary:

1. move the proven Jira policy/encoding code out of
   `kontor-integrations-asma` into one native `kontor-jira` crate and compose it
   directly into `kontord`; and
2. replace the realm-singleton memory switch with one closed authority ledger
   keyed by `(project_id, memory | backlog)`.

Do not add a connector framework, another workflow evaluator, a generic Jira
write endpoint or a second backlog model.

```text
typed /v1 intent + authority + idempotency
                    |
                    v
          kontor-daemon Services
          /          |           \
         v           v            v
   kontor-core   kontor-store   kontor-jira
   policy/spec   intents,       direct Jira REST
   reconciliation receipts      + refetch
         \           |            /
          \          v           /
           confirmed observations
           + project-scoped authority
```

The native connector owns transport only. `kontor-core` remains the sole Jira
field/workflow policy and reconciliation owner. `kontor-store` records every
intent before its first effect and confirms it only from a refetch. The daemon
derives all desired Jira values from stored Kontor facts and pinned specs; no
API or MCP body accepts an arbitrary field, status, assignee or comment.

`_tools/asma-cli` gets fail-closed compatibility behavior in OP-07. Until OP-08
publishes the matching Kontor CLI commands, legacy mutation modes refuse before
their Jira effect. They do not acquire a temporary HTTP client or a second
policy implementation.

## Verified baseline and gaps

The useful Jira implementation already exists:

- `kontor-core::ticket` owns the closed field projection, workflow specs,
  ownership rules, live-transition selection and typed conflicts;
- `kontor-integrations-asma::jira` owns canonical requests, exact field and
  workflow fixture compilation, observation interpretation, preview hashes,
  apply/refetch checks and receipt construction;
- the store already has task ticket links, immutable projections, observations,
  conflicts, comments and transition receipts; and
- OP-03 already exposes typed ticket claim/comment/conflict/reconcile routes.

The missing composition is narrow but currently total:

- `TicketDelegation` holds an `AsmaExecutable` and its `exchange` method runs
  `asma jira sync --request-json -`;
- `Services::reconcile_projection` uses literal `unknown` project/issue-type
  selectors and reports only whether a spec row is absent;
- `Services::ticket_reconcile_apply` records a command, then returns
  `Unavailable` for every non-empty diff without contacting Jira;
- `Services::pull_ticket_comments` does the same for every linked task;
- `Services::apply_epic` accepts caller-asserted ticket keys without live
  verification, and the scheduler deliberately permits an unlinked task;
- there is no epic-level Jira binding; and
- `_tools/asma-cli` still has direct writes in human sync/import, the legacy
  machine request, Git/ACP transition helpers and prompt interpretation.

Memory has the same single choke point: migration `0021_native_memory.sql`
creates one `memory_authority(singleton=1)` row, and every native memory write
calls a project-blind `require_authority(tx, "kontor")`. The global freeze
endpoint therefore changes every project at once and cannot represent backlog
authority.

## Crate and composition boundary

Create `crates/kontor-jira` by moving, not copying, the Jira module and its
contract fixtures out of `kontor-integrations-asma`.

`kontor-jira` contains:

- the existing pure specification catalog/compiler and request/response
  interpretation;
- one `JiraConnector` backed by the already-pinned `reqwest` dependency;
- a small secret-provider port with a system-keychain implementation and a test
  implementation; and
- direct create, lookup, observe, field update, assignment, transition, comment
  read and refetch operations.

The connector stores only validated project configuration and a credential
reference. It resolves the credential for each call, holds it in
`SecretString`, never serializes or debugs it, and drops it after the request.
Tests use the secret-provider port and `wiremock`; no general transport trait is
needed.

Leave `kontor-integrations-asma::{process,fleet}` in place for OP-08 to remove.
After the move it has no Jira module. `kontor-daemon` depends on `kontor-jira`
for both the shipped specs and the native connector, so there is only one Jira
implementation in the runtime graph.

### Supported configuration

Follow the existing state-root configuration pattern with one strict
`jira.json`, keyed by Kontor project id. Each entry contains:

- schema version;
- HTTPS endpoint;
- Jira project key; and
- opaque keychain alias;
- optional `create_fields.epic` and `create_fields.task` maps for additional
  operator-owned fields required by that project's Jira create screens.

Create-field defaults are configuration, never model-authored input. They may
not override the structural fields Kontor owns: `project`, `issuetype`,
`summary`, `description`, `labels`, or `parent`. The connector applies the
configured map for the selected issue kind and then writes its structural
fields. Missing Jira-required fields fail closed. Jira 400 responses expose
only bounded, syntactically safe field identifiers in diagnostics; Jira's
operator-facing error prose is not reflected through Kontor.

For the ASMA project, Task creation requires Product and has no Jira default.
The recommended local operator entry therefore includes:

```json
"create_fields": {
  "task": {
    "customfield_10251": { "id": "10459" }
  }
}
```

Option `10459` is ASMA's registered `Both` Product value. It is a project
configuration fact, not a portable Kontor default.

The fixed keychain service is owned by Kontor; the keychain value contains the
connector's authentication material. Reject duplicate projects, unknown keys,
non-HTTPS live endpoints, credentials in the JSON, redirects, off-origin
responses and oversized bodies. Test-only loopback HTTP remains explicit.

Configuration is operator-owned and never part of an MCP/model request.
`kontord` validates it at startup and reports only the project id plus a typed
configured/unconfigured state. Missing configuration produces `Unavailable`
before any receipt claims an external effect.

## Jira materialization and activation

Keep `epics:apply` tracker-neutral. Add two project/epic operations behind
`ApplicationOperations`:

| Operation | Contract |
| --- | --- |
| `POST /v1/projects/{project}/epics/{epic}/jira:preview` | Re-read the epic, tasks, desired projections, project Jira config and pinned specs. Accept only `create` or `link` intent per epic/task; a link names a key, while create names no caller-authored Jira fields. Return ordered effects and the canonical preview hash. Write nothing. |
| `POST /v1/projects/{project}/epics/{epic}/jira:apply` | Re-derive the preview, compare its hash and expected epic revision, persist stable per-item intents, execute/reconcile missing effects, refetch every item and return one batch receipt plus per-item receipts. |

The server derives project, issue type, parent, summary, body zones and owned
fields from the stored MiniProject/Tasks and compiled specs. The apply body
contains only the preview hash, expected revision and idempotency key carried by
the existing header convention.

One batch orders the Jira Epic first, then its Issues. Link mode refetches and
validates the supplied key before a binding exists. Create mode uses a stable
connector-owned marker derived from the persisted item intent. Before every
create or retry, query that marker:

- zero matches: create once;
- one match: validate and adopt it, including after a lost acknowledgement;
- more than one match: persist a typed conflict and write nothing else.

The marker is connector metadata, not Zone C prose, and is included in exact
readback. It makes an ambiguous create recoverable without trusting a local
timeout or producing a duplicate.

Persist the planned batch/item ids and marker before the first request. An item
becomes confirmed only after exact readback of key, project, issue type, parent,
owned fields, status and assignee. Retry a confirmed item by returning its
original receipt; retry an unfinished batch by processing only unfinished
items.

Add a confirmed epic Jira binding and confirmed task-link evidence rather than
treating the existing `jira_links` key as proof. Existing links migrate as
`legacy_unverified` and become confirmed only through native observation.

ASMA Epic activation is one final local transaction after all required
readbacks:

1. exactly one confirmed Jira Epic binding exists for the MiniProject;
2. every ASMA delivery Task has exactly one confirmed Jira Issue binding;
3. every Issue's parent is the confirmed Epic; and
4. no materialization item or ticket conflict is unresolved.

Only then record the ASMA policy activation receipt. Generic kernel
MiniProjects never need Jira. Scheduler readiness adds the confirmed-link check
only for an activated ASMA Epic; unactivated tracker-neutral work keeps the
existing Jira-neutral behavior. This preserves OP-REQ-026 without changing the
kernel aggregate.

## Existing ticket operations gain real behavior

Replace the placeholder implementations, not their routes:

- `ticket:reconcile-plan` loads the linked task, exact connector/project/issue
  specs and stored Kontor facts, then uses the native connector to observe the
  issue, principal and live transitions before calling the existing pure
  evaluator. Its preview hash covers the task/spec/projection revisions and
  exact observation.
- `ticket:reconcile-apply` re-derives the plan, refuses a stale hash, persists
  the item intent, re-observes, applies only the selected assignment/field/
  transition effects, refetches and stores the existing typed transition
  receipt. A status already at target is never transitioned twice.
- `ticket:pull-comments` pages Jira inbound comments, deduplicates by external
  id and canonical hash, persists provenance and never exposes an outbound
  comment field.
- ambiguity recovery observes first. It retries only when the desired effect
  is still absent; otherwise it confirms the first effect.

Keep field and workflow compilation in one place. Zone C is never presented to
`kontor-jira`; absent fields produce no request member; unknown/duplicate/
unmapped fields fail before transport.

## Project/subject authority ledger

Migration `0054_project_subject_authority.sql` replaces the singleton with
these closed facts:

```text
project_subject_authority
  project_id
  subject                 memory | backlog
  origin                  kontor_native | agentsroom_import_pending
  authority               kontor | agentsroom
  revision
  source_frozen_at?
  final_import_hash?
  readback_hash?
  switched_at?

subject_import_manifests
  project_id + subject + source + import_hash
  canonical_manifest + imported_count + readback_hash + imported_at

subject_authority_receipts
  id + project_id + subject + operation
  input_hash + result_hash + recorded_at
```

Rows and receipts are immutable except for the one guarded authority transition
from `agentsroom` to `kontor`. The store API always requires both project and
closed subject; there is no realm-wide overload.

Update `EnsureProjectRequest` to require both subject origins. Project creation
inserts the project and its two authority rows in one transaction and includes
the origins in the idempotency intent. Re-ensuring an existing root with
different origins is a conflict. Project readback returns both origins and
current authorities.

For migration of already-created Kontor projects:

- seed `backlog` as `kontor_native`, because their graph already lives in
  Kontor; and
- seed `memory` from the old singleton: `kontor` becomes `kontor_native`, while
  `agentsroom` becomes `agentsroom_import_pending`.

That preserves the Operational epic's one named memory bootstrap exception
without making its already-native backlog a second AgentsRoom writer. The
promotion Context Pack report is imported and switched through the new
project-scoped flow, then the exception expires by receipt.

Every native memory mutation calls
`require_authority(tx, project_id, Memory, Kontor)`. Every backlog mutation,
including epic/task graph creation and lifecycle changes, applies the same
check for `Backlog`. Reads and import previews remain available while pending.

### Fresh native subjects

A `kontor_native` row starts with `authority=kontor`. It can immediately create
backlog state and propose/approve memory. Freeze/import/switch is refused as
inapplicable; no empty export or global ceremony is synthesized.

### Legacy subject cutover

Memory and backlog keep separate typed import bodies but share the authority
ledger and receipt rules:

1. preview canonicalizes the supplied export and writes nothing;
2. apply imports every item transactionally and persists the manifest/hash;
3. readback recomputes a canonical hash from stored Kontor state, never from
   the submitted bytes;
4. a bounded operator attestation records that this project's source subject is
   frozen at the named source cursor/hash; and
5. switch requires the final import hash, exact stored readback hash and that
   attestation, then changes only the named `(project_id, subject)`.

Backlog import maps a closed export to the existing MiniProject/Task/dependency
graph and records legacy source ids in the manifest; it does not create a
parallel backlog table. Imported Jira keys remain unverified until the native
connector observes them.

The old realm-global freeze route becomes a typed refusal and cannot update any
row. Switching project A's memory cannot affect A's backlog or any subject in
project B.

## ASMA CLI cutover behavior

OP-07 removes direct Jira mutation before OP-08 adds forwarders. Use the
smallest safe interim rule: preserve read-only diagnostics and local Git/prompt
work, but refuse the Jira semantic effect at its shared command boundary.

Required behavior:

- human `asma jira sync` and `asma jira import` refuse before AgentsRoom or Jira
  mutation and print the Kontor replacement;
- `asma jira sync --request-json -` returns its legacy single-document envelope
  with a deprecated/unavailable outcome and performs no write;
- transition apply helpers refuse; transition reads and `asma doctor jira`
  remain read-only;
- Git/ACP commands may finish their ASMA-owned local work but cannot transition
  Jira directly; they return no fake Jira success;
- `asma prompt --write-ai-interpretation` may generate/print/copy the prompt,
  then refuses the Jira write; and
- create/update/transition transport functions retain a final fail-closed guard
  so a missed caller cannot restore a second writer.

Do not add a temporary Python Kontor HTTP client or a generic subprocess
forwarder in OP-07. OP-08 replaces these refusals with one-way `kontor <tool>`
forwarders after the corresponding CLI registry exists.

## Implementation checkpoints

1. Add the subject-authority types, migration, project-create origins, memory
   checks and mixed-project isolation tests. Keep the old global route as a
   refusal.
2. Move Jira policy/fixtures into `kontor-jira`, add strict configuration,
   keychain resolution and direct observe/apply/refetch contract tests. Remove
   Jira imports from `kontor-integrations-asma`.
3. Add durable Jira materialization batches/items, confirmed epic/task
   bindings and the preview/apply service. Activate ASMA policy only after full
   readback.
4. Replace the existing ticket reconcile/comment stubs with the native
   connector and reuse the current projection/conflict/receipt tables.
5. Add backlog import/cutover on the existing graph, gate native backlog writes
   by subject authority and prove restart/replay.
6. Add ASMA CLI refusals and static dependency/process checks, then import the
   Operational promotion context report through the deployed project-scoped
   memory flow and record its receipt.

Each checkpoint builds and leaves existing tracker-neutral kernel tests green.
Do not report a connector, cutover or activation success while any external
readback or local receipt is missing.

## Required proofs

- preview bytes/hash are the exact apply input; stale previews refuse;
- linked-key/project/type/parent/owned-field conflicts write nothing;
- create replay, lost acknowledgement and partial batch retry create no second
  Jira object and process only the missing suffix;
- direct `kontord` create, observe, apply and refetch work with `asma` absent
  from `PATH`;
- Zone C, absent fields, generic statuses/assignees/comments and credentials
  never enter a request, receipt, error, log or public schema;
- a Jira redirect or off-origin response receives no credential;
- ASMA activation requires one confirmed Epic and one confirmed Issue per
  delivery Task, while a Jira-neutral kernel MiniProject still runs;
- fresh native memory/backlog are writable immediately;
- a native project and a pending legacy project coexist in one realm;
- memory/backlog switches are isolated by both project and subject;
- legacy switch refuses missing freeze attestation, final import hash or exact
  readback hash;
- after switch, the imported project/subject has no AgentsRoom transport
  dependency and no legacy path can write it;
- all specified ASMA mutation modes either return a Kontor receipt later or
  fail now without the Jira effect; and
- dependency/call-graph scans find no Jira path from `kontord` to
  `AsmaExecutable`, `asma jira` or AgentsRoom.

## Out of scope

OP-07 does not build the generic Kontor CLI/MCP compatibility registry,
provider/fleet native collectors, realm-wide AgentsRoom read-only rollout,
diagnostic UI or command-removal machinery. OP-08 owns those surfaces. OP-07
leaves no direct Jira writer for them to preserve: it supplies the native
`/v1` intents and receipts they forward to.
