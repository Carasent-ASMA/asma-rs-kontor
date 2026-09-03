# KON-OP-22 implementation

Date: 2026-09-03

Status: delivered to `master` and verified live.

## Delivered behavior

- Jira convergence is resident. After the startup reconciliation barrier it
  scans confirmed task and epic bindings on committed control-plane wakes and
  on a bounded 30-second backstop.
- Task workflow selection is exact to the task's frozen work-profile revision.
  Epic workflow selection is generic and epic-specific; an epic never borrows
  a task profile.
- The Jira transport is entity-neutral and binds apply authority to the exact
  observed issue, live transition route, projection revision, specification
  hashes and intent. An applied result is accepted only with confirmed
  refetch evidence.
- Confirmed Jira task identities resolve through one canonical ledger. Legacy
  aliases remain migration input, not a second active identity.
- Epic conflicts and transition intents are first-class, append-only evidence
  with closed conflict kinds, timestamp and identifier checks, receipt
  references, and one-shot resolution.
- Replayed conflicts and failed external applies do not create an immediate
  self-wake loop; the bounded backstop owns retry.
- Jira milestone rules may declare an exact, acyclic status route. Kontor
  selects a hop only when the observed status matches one route source exactly,
  persists the actual hop destination in the intent, and requires exact
  destination readback before confirming it. Missing, ambiguous, cyclic or
  non-terminating routes fail closed.
- The generic ASMA epic workflow is revision 2. It owns the verified Jira route
  `New (10227)` -> `DRAFT (10237)` -> `TO BE GROOMED (10236)` ->
  `Groomed (10233)` -> `READY FOR DEVELOPMENT (10213)` ->
  `In Development (10214)`. Revision 1 remains readable and hash-stable for
  historical installations, but is no longer bundled as the current epic
  workflow.
- Completion creation or advancement, any derived forward profile, all TPM
  wake intents and the local command receipt commit atomically.
- Ticket work added or reopened after an epic leaves the ticket phase,
  including after `Done` or `NeedsHuman`, deterministically starts a new
  completion generation and returns the epic to `Tickets`. Prior integration,
  deliberation, remediation and closeout evidence remains immutable history
  attributed to its original generation.
- Startup and resident completion scans discover returned work without an
  operator command.
- Interrupted Jira materialization can resume an identical mixed Link/Create
  request, not only a link-only request. When legacy attempts split the exact
  ordinal set across several pending batches, recovery requires one complete,
  non-overlapping cover and retains each original batch in the immutable
  recovery ledger without rewriting item ownership. It never creates a
  replacement Jira issue merely to escape durable local history.
- Jira issue creation includes bounded, project-configured additional fields
  per issue kind while preserving Kontor's exclusive ownership of project,
  type, summary, description, marker labels and parent. This lets ASMA supply
  its required Product option without embedding an ASMA custom-field id in the
  generic connector.
- Jira 400 validation failures are classified as schema mismatch and report
  only safe rejected field identifiers. Jira error prose is never reflected.

## Schema

- v81: canonical Jira task-link ledger and exclusive active identity.
- v82: epic Jira conflict and transition-intent evidence.
- v83: completion-generation attribution for remediation proposals and replay
  claims, with generation-one backfill.

All migrations are append-only and covered by empty-database, deployed-lineage,
direct-SQL constraint, restart, export and preservation tests.

## Delivery receipt

- Implementation commit: `1951cac3102197ef46b5547be4f41088a1d42572`.
- PR: `https://github.com/Carasent-ASMA/asma-rs-kontor/pull/156`.
- Merge commit: `7c27f4d7a8e2aa37c1b1ddc576fe60387e95cf47`.
- The complete clean-archive verifier passed against the merge commit, including
  a byte-identical regenerated `Cargo.lock`.
- The live `kontor`, `kontor-daemon` and `kontor-mcp` hashes match the release
  build. LaunchAgent `com.asma.kontor.daemon` restarted as PID `18681` on
  `127.0.0.1:7717`; schema v83, `PRAGMA integrity_check = ok` and an empty
  foreign-key check were read back.
- Generic ASMA epic workflow revision 2 installed with receipt
  `01a067cf-beda-72c2-ac30-6042125a1f89`, project revision 5 and definition hash
  `21b1a100d832d688fbf99c4140f63aac8c8f7d9980aa1e7174288a3c2cf0c40e`.
- The resident controller moved `ASMA-8049` from `DRAFT` through the four
  remaining configured hops to `In Development`. All four distinct revision-2
  intents carry exact confirmed readback; a later backstop created no duplicate
  intent or conflict.
- `ASMA-8050` and `ASMA-8062` each return `converged: true` with an empty diff
  through the supported Kontor reconciliation plan.
- Historical revision-1 conflict
  `01a06761-49a2-7832-a11c-2b91e491a9a4` was resolved only after target
  confirmation, by receipt `01a067d0-6c17-7d02-a46d-602f57b1e5f3`.

## Explicit adjacent scope decision

The pre-existing open-question authoring/disposition API and MCP surface is not
part of KON-OP-22. Completion continues to read and enforce any persisted open
questions, and the live realm currently has none. This delivery neither removes
nor disguises that separate control-surface gap.
