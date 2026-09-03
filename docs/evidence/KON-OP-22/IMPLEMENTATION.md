# KON-OP-22 implementation

Date: 2026-09-03

Status: deterministic epic-route correction implemented, independently
approved and fully locally verified; merge and live promotion pending.

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

## Schema

- v81: canonical Jira task-link ledger and exclusive active identity.
- v82: epic Jira conflict and transition-intent evidence.
- v83: completion-generation attribution for remediation proposals and replay
  claims, with generation-one backfill.

All migrations are append-only and covered by empty-database, deployed-lineage,
direct-SQL constraint, restart, export and preservation tests.

## Explicit adjacent scope decision

The pre-existing open-question authoring/disposition API and MCP surface is not
part of KON-OP-22. Completion continues to read and enforce any persisted open
questions, and the live realm currently has none. This delivery neither removes
nor disguises that separate control-surface gap.
