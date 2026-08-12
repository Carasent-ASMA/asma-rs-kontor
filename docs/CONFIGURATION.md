# Configuration

Kontor separates invariants from deployment behavior. Rust enforces safety
properties such as one non-terminal session per role slot and uncertainty not
being completion. Names, durations, prompts, skills, profiles, teams, budgets
and runtime routing are versioned data.

## Seat supervision

Copy [`config/examples/paseo-supervision.yml`](../config/examples/paseo-supervision.yml)
to `<state-root>/supervision.yml` to enable the configured policy. If the file is
absent, Kontor invents no timeout or watchdog behavior.

Normal completion is notification-first: the orchestrator yields after dispatch
and the runtime wakes it on completion, error or permission. The watchdog is an
independent bounded observer for a turn that never completes. It may classify a
suspected hang only when both active-turn age and missing-progress evidence are
stale. Recovery reconciles the same seat first; it never duplicates a seat or
cancels running work.

The YAML contains prompt paths and required skill names. Kontor validates and
exposes those references but does not interpret their names; the selected
runtime adapter loads their contents. This keeps Paseo, AO, Codex and future
adapters on the same policy shape without hard-coded provider behavior.

## Other deployment data

- `runtimes.json` in the state root selects runtime families and endpoints.
- Profile packs define phases, gates, artifacts, budgets and runtime routing.
- Team packs define role slots, skills, contexts and handoffs.
- Account profiles contain non-secret provider-routing metadata.

Changing a prompt or duration changes configuration. Changing a safety
invariant requires an architectural decision and code review.
