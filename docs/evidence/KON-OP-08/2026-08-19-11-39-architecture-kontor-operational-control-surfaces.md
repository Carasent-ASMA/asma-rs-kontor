# Kontor operational control-surface architecture

> **Date:** 2026-08-19 11:39 CEST  
> **Status:** Approved for implementation  
> **Author:** Architect · KON-OP-08  
> **Category:** architecture  
> **Scope:** ASMA-7877 / KON-OP-08 — complete `/v1`/CLI/MCP parity, native
> operational ownership, replay-safe bootstrap/update and bounded ASMA
> compatibility  
> **Summary:** Complete the operational product around one `/v1` semantic
> boundary and one shared tool registry. Absorb the deliberately deferred OP-07
> Jira/backlog slice, compose OP-06's real observations, install the daemon and
> agent clients without secrets in config, then reduce ASMA orchestration modes
> to fixed Kontor subprocess forwarders before deleting the old ASMA integration
> crate.

---

## When to load

**Load this document when:**

- implementing or reviewing ASMA-7877;
- adding a public Kontor operation to `/v1`, CLI or MCP;
- implementing native Jira/backlog authority or completion observations;
- implementing the Kontor bootstrap/updater or an MCP client adapter;
- replacing an ASMA orchestration mode with a Kontor forwarder; or
- validating the Operational primary journey and legacy realm cutover.

**Do not load for:** ordinary kernel domain work, generic MCP tutorials or new
topology/workflow design. OP-08 closes existing Operational surfaces; it does
not invent another orchestration model.

## Decision

There is one semantic implementation and one public operation catalog:

```text
                     shared ToolSpec registry
                   /          |             \
                  v           v              v
              kontor CLI    kontor MCP   parity oracle
                  \           /              |
                   \         /               |
                    v       v                v
                 loopback /v1 <------ OpenAPI + router
                         |
                         v
              kontor-daemon Services
            /       |          |           \
           v        v          v            v
       core/store  runtime   native Jira   completion
       authority   adapters   connector     observations
```

`/v1` is the only semantic boundary. CLI and MCP validate against the same
`ToolSpec`, make the same loopback request and return the same response body.
Neither client owns workflow policy, Jira policy, topology inference, retries,
fallback semantics or a second response shape.

The bootstrap/update program is a local installation surface, not a semantic
Kontor tool and not an exception to registry parity. Ship it as a separate
binary target, `kontor-bootstrap`, in the existing `kontor-cli` package. Keep
`kontor <tool>` reserved for registry-derived `/v1` operations.

ASMA compatibility is a caller of the same generated CLI. Each retained ASMA
mode constructs one closed Kontor argv, passes Kontor stdout through unchanged
and branches only on the documented exit class. It never calls `/v1`, Jira,
Paseo, AgentsRoom or local fleet state directly and never implements a semantic
fallback.

## Reconciled baseline

OP-08 must begin from the integrated OP-04 through OP-07 baseline, not the
currently checked-out OP-03 submodule revision in this ticket worktree. Before
the first production change, bring in the accepted dependency revisions and
their accepted corrective branches, then run the full workspace tests. Resolve
migration numbers and generated snapshots only after that integration.

The dependency contract is:

| Slice | Reusable result | Remaining OP-08 composition |
| --- | --- | --- |
| OP-04 | Core Team, Quick, ECP/LSA/TPM, promotion and roster upgrade | Expose exact registry parity and include in the primary journey. |
| OP-05 | Native Advisor/Committee materialization, settlement and durable findings | Use settled native Committee runs as completion observations; no mock consultation port. |
| OP-06 | Completion profile compiler, state machine, receipts and bounded remediation | Replace `Unavailable` integration/closeout observation stubs with real durable records. |
| OP-07 | Project/subject authority schema and closed authority policy | Complete the intentionally deferred native Jira materialization, backlog import/cutover and ASMA refusals/forwarders. |

OP-07's release evidence explicitly re-scoped native Jira, backlog import and
ASMA forwarding out of that slice. OP-08 acceptance nevertheless requires Jira
create/link, legacy backlog retirement and zero ASMA subprocess dependency.
Those items are therefore inherited prerequisites of OP-08, not optional
follow-up work.

The current source still contains `kontor-integrations-asma`, `process.rs`,
`jira.rs`, `AsmaExecutable`, the daemon `--asma-executable` option and Jira
delegation through `asma jira sync --request-json -`. It also still returns
`Unavailable` while observing integration and closeout completion phases.
All of those are expected red-baseline facts.

## Public operation parity

### Registry is the client contract

Every public Operational route must have exactly one registry entry containing:

- stable operation/tool name;
- HTTP method and canonical path template;
- required credential tier;
- closed request schema;
- expected-revision and idempotency requirements;
- timeout class; and
- response/error schema identity.

Generate CLI subcommands and MCP tools from that entry. Do not maintain a CLI
match table or MCP-specific operation list. MCP capability filtering may hide
tools above the selected credential tier, but it may not change their schemas
or semantics.

Add one parity oracle that compares the router/OpenAPI operation inventory with
the registry and then drives every registry entry through both transports. The
only allowlisted non-tool endpoints are daemon health/readiness and local
bootstrap/update. A public route missing from the registry, a registry entry
missing from OpenAPI, or a CLI/MCP schema difference is a test failure.

The CLI prints exactly one JSON document to stdout for success and failure. It
has no `--json` flag and no progress/log prose on stdout. Logs go to stderr.
Preserve the established exit classes:

| Exit | Meaning | ASMA forwarder disposition |
| ---: | --- | --- |
| 0 | success | Return the Kontor document unchanged. |
| 1 | unexpected remote/internal failure | Terminal failure; no fallback. |
| 2 | local invocation/configuration failure | Terminal local failure; no fallback. |
| 3 | typed refusal | Return refusal unchanged. |
| 4 | conflict, stale revision or refetch required | Return retry/reconcile result unchanged. |
| 5 | unavailable/retryable dependency | Return unavailable result unchanged. |
| 6 | requested surface absent | Return absent result unchanged. |

### Missing semantic operations

Use the already accepted OP-07 Jira design rather than a generic connector API.
The minimum new public operations are:

| Operation family | Required contract |
| --- | --- |
| Epic Jira preview/apply | Project- and epic-scoped create/link intent; server-derived fields; canonical preview hash; expected epic revision; persisted per-item intents; exact refetch and replay. |
| Backlog import preview/apply/readback | Project- and subject-scoped manifest; canonical import hash; immutable receipt; exact readback hash; no realm-global switch. |
| Subject cutover preview/apply | One guarded `agentsroom -> kontor` transition after freeze/import/readback proof; no reverse transition. |
| Integration receipt record/read | A typed repository outcome linked to one successful integration TeamRun closure and its immutable evidence hash. |
| Closeout receipt record/read | One closed requirement (`merge`, `release`, `versions`, `summary`, `notification`, `archive`) plus a typed external or operator receipt. |

Names and paths must follow the repository's existing operation naming rules;
the table defines semantics, not permission to add aliases. Preview is
side-effect free. Apply/record operations require project scope,
`Idempotency-Key`, expected aggregate revision and a readback receipt.

Do not accept arbitrary Jira fields, statuses, comments, commands, native
runtime identifiers, topology JSON or closeout requirement names.

## Native Jira and backlog authority

Implement the accepted OP-07 architecture now:

1. Move the useful pure Jira compiler/encoding/observation code out of
   `kontor-integrations-asma` into `kontor-jira`; do not copy it.
2. Keep desired Jira fields, workflow policy and reconciliation decisions in
   `kontor-core::ticket`.
3. Put direct HTTPS transport, bounded response handling, secret resolution
   and refetch interpretation in `kontor-jira`.
4. Persist intent before the first external effect and confirm only from exact
   refetch. Stable markers recover ambiguous create acknowledgements without
   creating duplicates.
5. Derive every epic/issue value from stored Kontor facts and pinned specs.
   Callers choose only the closed `create` or `link` intent; link is verified
   before it becomes a confirmed binding.
6. Configure Jira per Kontor project with an opaque keychain reference. Resolve
   credentials at call time into secret-wrapped memory. No secret may appear in
   a request, receipt, config dump, argv, MCP config, log or error.

Backlog authority uses OP-07's project/subject ledger. Import is
project-scoped, replay-safe and hash-addressed. Public project creation remains
refused while legacy backlog authority is pending. Cutover becomes legal only
after source freeze, final import, exact readback and zero unresolved conflicts;
the authority transition is one-way.

Delete `kontor-integrations-asma`, `process.rs`, `AsmaExecutable`,
`TicketDelegation`'s ASMA executable field, the daemon `--asma-executable`
option and every `Command::new("asma")`/equivalent only after native Jira and
backlog tests are green. There must be no feature-flagged or test-only ASMA
subprocess dependency left behind.

## Completion observation assembly

Do not change OP-06's scheduler state machine or accept caller-authored
`CompletionObservation` values. Compose observations from durable domain facts:

### Integration and remediation

A typed integration receipt cites:

- the completion run and expected revision;
- the exact successful integration TeamRun;
- that TeamRun's immutable terminal evidence hash;
- an ordered, non-empty set of repository/module outcomes;
- PR or equivalent integration reference and delivered module revision; and
- a root-pointer revision where that repository participates in the polyrepo.

The daemon verifies project/epic/task ownership, the pinned TeamTemplate, the
successful terminal outcome, evidence hash and uniqueness before storing the
receipt. `completion:advance` derives `IntegrationCompleted` or
`RemediationCompleted` from that stored receipt. A successful TeamRun alone is
not enough to invent repository revisions.

### Committee

Keep the existing native observation path: only a settled Committee run with
the pinned template, expected round, immutable result, findings and evidence
digest produces `VerdictRecorded`. A native session finishing and a caller
claiming a verdict are never evidence.

### Closeout

Record each closeout prerequisite through a native connector receipt where one
exists, otherwise through a typed operator receipt authorized for that exact
requirement. The record operation validates the referenced immutable artifact
and stores provenance. `completion:advance` reads the accumulated receipts in
the fixed scheduler order and derives `CloseoutRecorded`.

An archive receipt must include exact readback of the archived Kontor/runtime
identities. A notification receipt records a bounded delivered/skipped/failed
outcome; it cannot silently coerce failure to success. Replay returns the
original receipt, and a different body under the same idempotency key conflicts.

## Bootstrap and update

### Local contract

`kontor-bootstrap` is non-interactive and agent-runnable. It accepts an explicit
state root and install root, defaults only to documented user-scoped locations,
and emits one JSON result with a disposition for every step and every supported
client. It must support:

- clean install;
- update over an older installation;
- replay of the same version/configuration;
- repair after a missing/stopped daemon or partially written client entry; and
- explicit refusal on an incompatible or ambiguous existing entry.

Install `kontor`, `kontord`, `kontor-mcp` and `kontor-bootstrap` atomically from
one versioned artifact set. Record binary versions and digests. Install a
durable user service using launchd on macOS and systemd user services on Linux;
unsupported platforms return a typed per-step result rather than pretending
durability. Never require `sudo`.

After install/update, restart the same service identity and prove:

1. daemon process/service readback;
2. health and readiness;
3. daemon version and schema compatibility;
4. exact tool-registry count/hash from the installed `kontor-mcp`; and
5. one capability-filtered MCP initialize/list-tools exchange.

Do not report success from file copies alone. Persist a bootstrap receipt under
the state root so replay can distinguish already-applied, repaired and changed
steps.

### Credential handling

Register MCP as:

```text
<absolute install path>/kontor-mcp
  --state-root <absolute state root>
  --credential-tier admin
```

The tier selector and state-root path are not secrets. The selected credential
stays in the mode-0600 Kontor credential store and is loaded by `kontor-mcp` at
runtime. Never place its value in a client config, environment block, argv,
receipt or test snapshot.

### Client adapters

Use explicit adapters, not a plugin/configuration framework. Patch only the
`kontor` entry, preserve all unrelated settings and read the entry back using
the client's supported surface where available.

| Client | Authoritative user-scoped shape | Readback rule |
| --- | --- | --- |
| Codex | `~/.codex/config.toml`, `[mcp_servers.kontor]`, `command` plus `args` | Parse the TOML and, when installed, confirm with `codex mcp list`. Codex CLI, IDE and desktop share this host config. |
| Claude Code | Native `claude mcp add-json kontor --scope user ...` | `claude mcp get kontor`; do not hand-edit an undocumented user store. |
| OpenCode | Current global `~/.config/opencode/opencode.json(c)`, `mcp.servers.kontor`, local command array | Parse the installed client's supported schema and confirm with its MCP list command; refuse an unsupported legacy schema rather than guessing. |
| VS Code/Copilot | Portable user `~/.copilot/mcp-config.json`, `servers.kontor` stdio entry | Structured JSON readback; VS Code's Agent Host reads this file natively. |
| Cursor | `~/.cursor/mcp.json`, `mcpServers.kontor` | Structured JSON readback and, when installed, Cursor MCP list/tools readback. |

An adapter result is one of `installed`, `updated`, `already_current`,
`repaired`, `absent`, `conflict` or `failed_readback`. A missing client is
`absent`, never a silent success and never an overall failure when every
present client was configured. A same-name unrelated entry is `conflict` and
must not be overwritten without an explicit repair input that cites its
observed hash.

Tests must prove preservation with fixtures containing unrelated tables,
objects, arrays, comments where the format supports them and paths containing
spaces. Replay must be semantically and, for untouched sibling regions,
textually stable.

Client-format references used for this decision:

- [Codex MCP configuration](https://learn.chatgpt.com/docs/extend/mcp)
- [Claude Code MCP configuration](https://docs.anthropic.com/en/docs/claude-code/mcp)
- [OpenCode MCP servers](https://opencode.ai/v2/docs/mcp-servers)
- [VS Code MCP configuration](https://code.visualstudio.com/docs/agents/reference/mcp-configuration)
- [Cursor MCP configuration](https://docs.cursor.com/context/model-context-protocol)

## Bounded ASMA compatibility

Inventory and lock every orchestration-relevant ASMA mode before editing. The
accepted dispositions are:

| ASMA surface | OP-08 disposition |
| --- | --- |
| `asma jira sync` | Deprecated fixed forwarder to the matching native Kontor operation. |
| `asma jira sync --request-json -` | Deprecated legacy-schema translator/forwarder; no direct Jira. |
| `asma jira import` | Deprecated forwarder to project-scoped backlog preview/apply/cutover; no direct AgentsRoom/Jira. |
| `asma fleet preflight`, usage, status, block/unblock, verify-pins, reap, record, report | Deprecated fixed forwarders to named Kontor tools. |
| `asma fleet watch self/seats/stale` | Forward only closed typed scheduler/watch inputs with a finite bound; reject arbitrary argv or shell behavior. |
| `asma git branch create/checkout/commit/acp --transition` | Retain local Git/PR behavior; forward only the Jira/Kontor transition effect. |
| `asma jira transition-list` | Deprecated read-only forwarder over native Jira observation. |
| `asma doctor jira` | Retain as a direct read-only operator diagnostic. |
| `asma prompt --write-ai-interpretation` | Forward the write; prompt rendering remains non-orchestration. |
| scaffold orchestration flags | Non-orchestration here; OP-11 owns wording/removal. |
| every other top-level mode | Explicitly classified non-orchestration or covered by a named test. |

Do not invent `asma fleet probe`, a new command group, a generic `kontor raw`
forwarder or fallback to the old implementation. Centralize subprocess setup
in one narrow `_tools/asma-cli` helper with fixed argv builders, a bounded
timeout, inherited stdout, captured/diagnostic stderr and environment
allowlisting. The helper does not parse a successful Kontor document to make a
second decision; the exit class is the decision boundary.

Every retained mode gets a contract test for exact argv, stdout identity,
exit-class propagation, missing binary, timeout, broken pipe and malformed
legacy input where applicable. Every deprecated mode emits its deprecation in
the one Kontor JSON response or stderr without adding a second stdout document.

## Realm cutover and reassurance refusal

Keep legacy realms read-only until OP-REQ-022's per-realm readiness evidence is
complete. Cutover is explicit, previewed, revision-checked, idempotent and
read back from the authority ledger. It must not be inferred from the presence
of a Kontor daemon or a successfully installed client.

Enforce OP-REQ-036 at the daemon's semantic continuation boundary. After a
failed readiness/completion gate, reconciliation conflict, exhausted bounded
remediation or ambiguous external effect, the only continuation is the
existing Gate or `NEEDS_HUMAN` path. CLI, MCP and ASMA forwarders must expose
that same typed refusal. No client may offer a reassurance/continue override or
translate the refusal into success.

## Test-first checkpoints

Each checkpoint is a coherent, reviewable ASMA commit. Start each with the
smallest behavior-level red test and keep the workspace green before moving on.

### Checkpoint 0 — integrate and freeze the inventory

- Integrate accepted OP-04 through OP-07 plus their required corrective
  revisions into this worktree.
- Run the complete Rust workspace suite and relevant ASMA CLI suite.
- Commit generated inventories of public `/v1` operations and ASMA invocation
  modes as test fixtures, not duplicated production registries.
- First red test: the parity oracle demonstrates the currently missing
  registry/routes required by the accepted OP-08 narrative.

### Checkpoint 1 — native Jira/backlog ownership

- Implement epic Jira preview/apply, exact refetch and replay.
- Implement backlog import/readback and guarded subject cutover.
- Replace ticket reconcile/comment placeholders with the native connector.
- Remove the Jira module from the ASMA integration crate while keeping the
  still-needed fleet compatibility temporarily.
- Mutation proof: sabotage post-effect refetch or stable-marker recovery; the
  create/replay tests must fail.

### Checkpoint 2 — completion assembly and full registry parity

- Add typed integration and closeout receipt operations.
- Compose TeamRun, Committee and closeout observations into completion advance.
- Fill every public operation's registry entry and make OpenAPI/CLI/MCP parity
  green across admin and narrow credential tiers.
- Mutation proof: accept a mere successful TeamRun or caller verdict; the
  completion tests must fail.

### Checkpoint 3 — bootstrap/update and client registration

- Implement atomic binary installation and durable service management.
- Implement the five explicit client adapters and per-client dispositions.
- Prove clean install, update, replay, repair, absent-client handling,
  unrelated-config preservation and exact daemon/registry readback.
- Mutation proof: skip one readback or leak a credential into a fixture; the
  bootstrap/security tests must fail.

### Checkpoint 4 — ASMA forwarders and deletion

- Replace every accepted ASMA orchestration mode with its fixed forwarder or
  retained-local classification.
- Add exact argv/stdout/exit/timeout/broken-pipe tests.
- Delete `kontor-integrations-asma` and every remaining ASMA subprocess path.
- Run zero-reference scans over source, manifests, fixtures and built
  dependency graphs.
- Mutation proof: enable a fallback after exit 3/4/5 or alter stdout; the
  forwarder tests must fail.

### Checkpoint 5 — primary journey and cutover proof

- From installed MCP only, exercise bootstrap readback, project/epic creation,
  Jira create/link, backlog authority, Core Team/Quick, delivery TeamRun,
  Advisor/Committee, completion, remediation, final closeout and archive
  verification.
- Run it once with admin authority and prove a narrow credential cannot list or
  call forbidden tools.
- Run install/update/replay for every present client and assert explicit absent
  dispositions for the others.
- Exercise legacy-read-only, cutover preconditions and OP-REQ-036 refusal.
- Preserve machine-readable transcripts and exact version/registry hashes for
  QA and release review.

## Required verification

The implementation is not ready for review until all of these are green:

- `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features`
  and `cargo test --workspace --all-targets` on the integrated Rust tree;
- the ASMA CLI unit/contract suites for every mapped mode;
- OpenAPI/router/registry/CLI/MCP equality tests;
- capability-tier tool-list and forbidden-call tests;
- native Jira exact-request, refetch, ambiguity and replay tests with no live
  secrets;
- backlog import/readback/cutover replay and project-isolation tests;
- completion integration/Committee/closeout observation tests;
- bootstrap clean/update/replay/repair/client-preservation tests;
- installed daemon restart plus installed MCP initialize/list-tools readback;
- zero references to `AsmaExecutable`, `kontor-integrations-asma`,
  `--asma-executable`, `Command::new("asma")` and local fleet/AgentsRoom state;
- an `asma`-absent primary Kontor journey; and
- deliberate mutations for Jira refetch, completion evidence, registry parity,
  credential secrecy and ASMA no-fallback behavior.

## Non-goals and refusal lines

- No second semantic client, connector framework, workflow engine or topology
  model.
- No generic HTTP/MCP proxy, arbitrary Jira field API or caller-authored
  `CompletionObservation`.
- No `--json` flag, multiple stdout documents or prose success output.
- No raw Paseo/AgentsRoom/Jira mutation from CLI, MCP or ASMA compatibility.
- No secrets in configuration, environment blocks, argv, receipts, logs or
  snapshots.
- No realm-global authority switch and no automatic legacy cutover.
- No new ASMA command families or temporary semantic fallback.
- No deletion of a legacy path until its native behavior and replay proof are
  green.

## Evidence sources and known runtime limitation

This handoff reconciles the accepted OP-04 through OP-07 architecture,
implementation, QA and release evidence; the OP-08 plan contract; the
integrated source registry and completion implementation; and current official
MCP client configuration contracts. The architect session timeline itself
returned the already-recorded `timeline_refetch_required` failure documented in
`_docs/ai-orchestration/reports/2026-08-19-10-40-report-kontor-op07-timeline-refetch-gap.md`.
No direct Paseo fallback, replacement seat or duplicate operational-gap report
was used. Durable Kontor task/dependency reads and repository evidence remained
available and were used instead.
