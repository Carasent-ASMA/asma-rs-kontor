# Kontor MCP seats

One process, one realm, one authority. A seat is which of a realm's three secrets
the process holds — and that is the whole boundary. There is no per-call authority,
no escalation argument and no per-role policy inside `kontor-mcp`: running at two
authorities means running two servers.

The three files here are the seat configurations. Only `paseo-lead.json` is
admin-scoped.

| Seat | File | Tier | Reaches |
| --- | --- | :---: | --- |
| Paseo Lead Architect | [`paseo-lead.json`](paseo-lead.json) | `admin` | The whole tool vocabulary |
| Worker | [`worker.json`](worker.json) | `operator` | The registry's `worker` serve profile: reads of its own work, claiming, turn settlement, gates, session follow-up, intake, memory proposals and context |
| Reviewer | [`reviewer.json`](reviewer.json) | `observer` | Reads only |

## Filling in the two values

`--state-root` is the directory the daemon was started with: it holds `kontor.db`,
the lock and the `0600` `credentials.json` this server reads. `--base-url` is only
needed when the realm is **not** on its default loopback port — the daemon's
`endpoint.json` is read when present, and `http://127.0.0.1:7717` is the default
otherwise.

Both come from the installed `kontord` configuration. Neither is a new MCP concept,
and neither may name a non-loopback address: an address that does is refused at
startup, before a client exists.

## What a seat file may not contain

- **A bearer value.** The secret is read from the realm's own credential file and
  never appears on argv, where every process listing on the machine would show it.
- **An arbitrary URL.** Only loopback is addressable.
- **A free-form tool subset.** The tool list follows from the tier. A seat file
  that listed tool names would be a second authority model beside the credential:
  config drift across seat files, no single source of truth, and a list readers
  would mistake for authority when only the credential enforces anything. What a
  seat *may* name is a **serve profile** — `--serve-profile worker` — because a
  profile is declared in the registry next to the tiers, not in the seat file.
  A profile narrows presentation only, always within the credential tier, and is
  enforced at call time as well as in the tool list: a tool the profile excludes
  is refused even when the tier would allow it, so the list and the callable set
  are the same set. A profile can never widen a tier, and an unknown profile name
  refuses to start. Free-form lists remain banned.

## Why the worker is not the Lead

A worker needs to answer a permission prompt, send a follow-up, record a gate
verdict, settle a run and reconcile a ticket. None of those is a credential,
account or policy-authority decision, so none of them needs admin. What admin adds
is exactly the set a worker must not have: creating projects and account profiles,
applying an epic graph, arming and disarming execution, correcting a selection, and
waiving a gate.

Admin is not a domain bypass either. The daemon still enforces revisions,
idempotency, admission, runtime capabilities, evaluator roles, evidence and closure
gates against the Lead's credential exactly as it does against a worker's.
