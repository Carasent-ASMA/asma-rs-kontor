# Live Fleet Routing Instead of Frozen Template Routes

> **Date:** 2026-09-24 20:45
> **Status:** 🟢 Approved
> **Category:** architecture
> **Scope:** `asma-rs-kontor` model routing for delivery, Committee and Advisor seats; the live Kontor state root `<state-root>/fleet.yml`; the asma-modules plan `2026-09-24-07-30-plan-kontor-live-fleet-configuration.md`
> **Summary:** Every seat placement resolves its model route from one owner-only `fleet.yml` in the live state root, read at placement time, instead of from the route frozen into a published template revision. Editing that file changes the next placement with no rebuild, restart or template republish; an invalid edit keeps the last valid version. This supersedes template-snapshot immutability **for routing only**.

---

## When to Load

**Load this document when:**

- changing how a delivery, Committee or Advisor seat chooses its model route;
- adding, removing or reorganising a model, account, provider or chain in the ASMA fleet;
- deciding whether a routing input belongs in `fleet.yml`, a template revision, or the compiled catalog;
- reviewing why a seat did not use the route its template declared.

**Do NOT load for:** seat permission posture (see `docs/CONFIGURATION.md`), workflow shape and phase gates, naming and hierarchy, or Jira field mapping.

---

## Context

On 2026-09-23 all six open ASMA epics stopped. Codex was out of quota, the
default `claude` login had expired, and the live fleet still had working models —
but Kontor could not reach them. Four causes, one shape:

1. the repo manifests shipped one-model chains that were loaded as the fleet pack;
2. a chain is copied into every template version, and every run freezes its copy
   at start, so a better chain helps only new runs after a repoint;
3. the model list was a hard-coded `match` in the daemon, so adding a model meant
   a rebuild and a deploy;
4. Cursor was refused on consultation seats, leaving Committees with no provider.

PR #267 (the bridge) moved individual stuck seats by hand. This decision removes
the need to.

## Decision

- **DEC-001:** The fleet configuration is exactly one file,
  `<state-root>/fleet.yml`, owned by the state-root user and mode `0600`. It is a
  regular file, at most 256 KiB, UTF-8, with unknown fields rejected.
- **DEC-002:** Every placement asks the fleet source for the current snapshot:
  seat fill, seat replacement, quota takeover, succession refresh, Committee
  invoke, Committee seat recovery after a provider loss, and Advisor invoke. The
  source compares the file's stamp (length, modification time, device, inode)
  and re-parses only when it changed. An edit takes effect at the next placement;
  no restart, deploy or template republish is needed.
- **DEC-003:** An invalid edit never applies in part: Kontor keeps the last valid
  snapshot and reports the failing rule in `<state-root>/fleet-status.json`. Each
  accepted version is stored once under `<state-root>/fleet-history/<hash>.yml`.
- **DEC-004:** A missing `fleet.yml` means legacy behaviour: every placement
  behaves exactly as before this decision. Deleting the file is the supported way
  to turn the feature off.
- **DEC-005:** A chain is an ordered list of steps; a step is one failure domain
  (a group of accounts that go down together); models and same-provider accounts
  inside a step are sub-steps. Kontor walks every model on every account of a
  step before descending to the next domain.
- **DEC-006:** A binding maps `team/<template_id>/<slot>`,
  `committee/<template_id>/<slot>` or `advisor/<profile_id>` to a chain and
  applies to every version of that template or profile. A binding wins over the
  frozen template chain; a bound chain that flattens to no admissible route fails
  closed rather than falling back. `core/<role_code>` keys are rejected in v1.
- **DEC-007:** Reviewer independence is **vendor**-based: when the fleet lists a
  route, its vendor (anthropic, openai, xai, zhipu, deepseek, cursor) is the
  diversity key, so Opus through Cursor still counts as Anthropic. Without a
  fleet the old provider-family key still applies. A route whose vendor is
  `unknown` (Cursor Auto) can never fill a reviewer slot.
- **DEC-008:** `rules.independent_of` makes a verdict seat avoid the vendor the
  named seat last ran on in the same team run, reading the per-placement decision
  log. When the other seat has no recorded route, placement proceeds and logs
  `fleet.independence_unknown`.
- **DEC-009:** Receipts are files under the state root — `fleet-status.json`,
  `fleet-history/` and `fleet-decisions/<team_run_id>.jsonl` — not database rows.
  No migration, no OpenAPI change and no new MCP tool: an older binary refuses a
  newer schema, so a migration would make rollback unsafe for an urgent fix.
- **DEC-010:** Only placement paths see the fleet. Template publishing, the
  selection of Committee revisions eligible for reopen, and restart-time runtime
  configuration keep the compiled catalog, so editing the file cannot invalidate
  a published template or make a Committee unreopenable.

## What stays frozen

- **DEC-011:** A running session is never switched mid-turn. Existing team runs
  and Committee runs use the fleet chain at their **next** placement.
- **DEC-012:** The Committee admission record stays immutable: the routes and
  provenance frozen at invoke are the run's record, and the fleet hash is stored
  as their evidence.
- **DEC-013:** Workflow shape, phase gates, completion rules, seat permission
  posture and native naming remain template- or code-owned. This decision changes
  routing, and routing only.

## Rationale

- **RAT-001:** Routing is an operational input that changes with vendor
  availability, and vendors change far more often than Kontor ships.
- **RAT-002:** A model list compiled into the binary made every model addition a
  rebuild and a deploy; the snapshot's own list is now authoritative for
  placement, while `/v1/catalog` stays the compiled surface.
- **RAT-003:** A stat on each placement is cheaper than the placement itself and
  cannot miss an event, so no file watcher or background task is needed.
- **RAT-004:** Files under the state root avoid a schema migration, which keeps
  rollback to a previous binary safe during an urgent fix.

## Consequences

### Positive

- **POS-001:** One edit moves the next placement onto a different vendor with no
  deploy; an invalid edit is visible in `fleet-status.json` and changes nothing.
- **POS-002:** Reviewer independence follows the model's maker, not the harness,
  so two providers can no longer seat two reviewers on the same vendor.
- **POS-003:** Every accepted version and every admitted placement is auditable
  from files beside the configuration.

### Negative

- **NEG-001:** A valid but poor file is accepted; that is operator
  responsibility, and rollback is one copy from `fleet-history/`.
- **NEG-002:** Receipt files grow without bound (retention is deferred).
- **NEG-003:** `fleet.yml` is outside version control by design, so it needs its
  own backup and review discipline.

### Mitigations

- **MIT-001 (NEG-001):** The shipped example is byte-equal to the plan's
  Appendix A and is parsed by a unit test; the removal order (chains first,
  `models` only when no live seat is frozen on the route) is documented in
  [`docs/CONFIGURATION.md`](../../docs/CONFIGURATION.md).
- **MIT-002 (NEG-002):** Retention is listed in the deferred set below.
- **MIT-003 (NEG-003):** The state root is backed up with the database before a
  release, as the deployment procedure already requires.

## Alternatives Considered

### Keep moving seats by hand with the bridge (PR #267)

- **ALT-001:** Rejected: it moves one seat at a time, needs an Admin for every
  move, and does nothing for Committees or for new runs.

### Raise the four-rung cap, republish templates and re-point work profiles

- **ALT-002:** Rejected: chains stay frozen per run, so in-flight runs never see
  the fix, and every later model change needs another publish cycle.

### Database-backed fleet configuration with API and console editing

- **ALT-003:** Rejected for v1: it needs a migration, which makes rollback
  unsafe for an urgent fix; it is the right long-term home and is deferred.

### A file watcher and in-memory cache

- **ALT-004:** Rejected: it adds a dependency and a background task. A stat on
  each placement is cheaper and cannot miss an event.

### A process-wide global snapshot

- **ALT-005:** Rejected: the loopback tests run several daemons in one process,
  so the snapshot source is a `Services` field instead.

## Deferred

- **DEF-001:** Database receipt table for fleet decisions, `GET /v1/fleet`, MCP
  `kontor_fleet_get`, and console editing.
- **DEF-002:** Templates published without model chains (fleet as the only
  routing source).
- **DEF-003:** Fleet sections for capacity, supervision, takeover, permissions,
  workflow shape and naming.
- **DEF-004:** `GET /v1/catalog` lists fleet routes.
- **DEF-005:** `core/<role_code>` bindings for epic leadership seats. Stage 6
  found three distinct explicit-route selection sites (initial materialization,
  route preview, intent supersession) whose request contracts would have to
  change, so the validator keeps rejecting `core/` keys.
- **DEF-006:** Quota tracked per account and model prefix, so the `deepseek` and
  `openrouter` domains (both on the `opencode` account) fail independently.
- **DEF-007:** Retention for `fleet-decisions/` and `fleet-history/`.

## References

- [`docs/CONFIGURATION.md`](../../docs/CONFIGURATION.md) — operator guide for the
  file, its schema, the three rules, readback and the removal order.
- [`config/examples/fleet.yml`](../../config/examples/fleet.yml) — the shipped
  example, also the first live file.
- [Plan: Kontor Live Fleet Configuration Implementation Plan](https://github.com/Carasent-ASMA/asma-modules/blob/master/_docs/ai-orchestration/plans/2026-09-24-07-30-plan-kontor-live-fleet-configuration.md) —
  the binding implementation plan and its decisions DEC-001…DEC-017.
- [`docs/QUOTA-FALLBACK-PLAN.md`](../QUOTA-FALLBACK-PLAN.md) — how quota evidence
  and takeover interact with a declared chain.
- [PR #267](https://github.com/Carasent-ASMA/asma-rs-kontor/pull/267) — the
  bridge commands this decision makes unnecessary for ordinary routing.
