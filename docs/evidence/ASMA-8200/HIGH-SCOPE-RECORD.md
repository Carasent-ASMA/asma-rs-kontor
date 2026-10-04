# ASMA-8200 high-scope record

Date: 2026-09-17
Artifact: `high-scope-record`
Task: Jira `ASMA-8200` / Kontor `01a0ac9d-a95c-7291-91e8-b472b7b331bc`
Epic: Jira `ASMA-8190` / Kontor `01a0abdd-b84e-7be2-a1f6-b9f34aae8c23`
Project: `01a0064a-e056-7603-9968-ef64fdaacb75`
Phase: `high-scope`
TeamRun: `01a0b033-78bd-7691-90e0-a8a34eb46030`

## Decision

Activate a persisted capacity configuration when a daemon starts. Read the
singleton row once, after the store has opened and migrated and before
`Services` is composed. A present stored configuration is authoritative; the
`DaemonConfig` capacity remains the seed used only when no row exists. Refuse
startup if a present document cannot deserialize or fails `CapacityConfig`
validation.

Do not add live reload. Admission plans and the `Services` capacity are
process-lifetime state today, while the existing apply/read contract already
exposes `restart_required`. Restart is therefore the smallest supported
activation boundary that cannot split one process between two policies.

No schema, route, CLI, MCP, or DTO addition is required. Keep the existing
configuration revision and compare-and-swap write. `capacity-config-get`
continues to report the effective policy in `ceilings`, reports a differing
durable replacement in `stored_ceilings`, and sets `restart_required` exactly
when those two policies differ. An apply of values already in force must not
claim a restart is required.

## Evidenced root cause and live baseline

The canonical worktree is
`asma-modules/.worktrees/asma-8200/asma-rs-kontor`, branch
`feat/ASMA-8200-enforce-persisted-capacity-configuration-at-runtime`, clean at
`2e4b9985fc1073dd25c22b78349d8f36d6b1d4ec` when this record was written.

The executable constructs `DaemonConfig::at(state_root)` in
`crates/kontor-daemon/src/main.rs`; that seeds `DEFAULT_CAPACITY`.
`Daemon::start_configured` builds the fleet and delegates to
`start_with_supervision`. That function opens the SQLite store but never calls
`SqliteStore::get_capacity_configuration`; it passes `config.capacity`
unchanged into `Services::new`. `Services` retains that one `CapacityConfig`
and every scheduler projection and admission path reads it. The only repository
reader is currently called by the configuration read surface, after composition.

Live readback before implementation proves the drift:

| field | effective | stored revision 1 |
| --- | ---: | ---: |
| `global_max_in_flight` | 24 | 20 |
| `project_max_in_flight` | 14 | 13 |
| `mission_max_in_flight` | 12 | 12 |
| `runtime_max_in_flight` | 14 | 13 |
| `adaptive.ceiling` | 12 | 12 |

`kontor_capacity_config_get` returned `restart_required: true` at snapshot
cursor 3933. SQLite holds the same stored values at revision 1, updated
`2026-09-16T21:50:48.955532Z`. A prior full daemon restart retained the drift,
as recorded in approved gap `OG-054`.

## Implementation boundary

ASMA-8200 owns:

1. one startup loader at the composition root, reusing
   `SqliteStore::get_capacity_configuration` and the existing wire-to-domain
   conversion;
2. fail-closed startup handling for a present unreadable or invalid stored
   policy;
3. truthful `restart_required` reporting when apply writes values already in
   force;
4. focused restart and mutation regressions;
5. one guarded rebuild/redeploy of this same fleet and exact post-restart
   identity/capacity readback.

It does not own new capacity knobs, changing `DEFAULT_CAPACITY`, hot reload,
per-project capacity, adaptive-window semantics, configuration history, schema
changes, or a second scheduler configuration path. Do not duplicate the
capacity DTO/domain conversion or teach the store scheduler semantics.

## Required regression and mutation evidence

Leave one focused restart regression that:

1. starts a realm without a stored row and proves the composed seed remains in
   force;
2. applies a complete, valid policy different from the seed and proves the
   running process still reports the old effective policy plus
   `restart_required: true`;
3. stops and reopens the same state root through the shipped startup path;
4. proves the realm identity and configuration revision are unchanged, the
   effective values now exactly equal the stored values,
   `restart_required: false`, and no differing `stored_ceilings` remains;
5. proves the scheduler-facing capacity projection uses the reopened values,
   not merely that the configuration endpoint echoes them.

Keep the existing compare-and-swap/replay regression green. Add a startup
refusal check for a present policy that cannot be converted or validated only
if the condition can be created through an isolated test store without
weakening the production write boundary.

Mutation: remove or bypass the persisted-policy override at startup. The new
restart regression must fail on effective readback and scheduler projection;
restore the production code and rerun green. Record the exact mutation and
failure in the `high-change` artifact.

Minimum implementation checks are `cargo fmt --all --check`, the exact new
restart regression, the existing capacity-configuration regression,
`cargo test -p kontor-daemon --lib`, and `cargo check -p kontor-daemon
--all-targets`. Independent verification owns the broader suite and contract
parity reruns.

## Deployment and convergence proof

Before deployment, take a verified database snapshot and record the current
realm, project, epic, task, TeamRun, AgentRun, SeatBinding and runtime-binding
identities/counts. Rebuild and deploy the same fleet with exactly one governed
restart. After restart:

- the running binary must read back as the reviewed candidate;
- realm `01a00649-9ee6-73e0-ba1b-6a6c35cfd065`, this project, epic, task and
  TeamRun remain the same identities;
- the configuration remains revision 1 and effective readback is exactly
  `20/13/12/4/4/13`, adaptive `4/1/12/1`, with no headroom;
- `restart_required` is false and no differing `stored_ceilings` is returned;
- scheduler/project capacity readback agrees with mission ceiling 12; and
- no task, run, seat, native session, topology, gate or Jira state is created,
  replaced, settled, transitioned or otherwise changed by activation.

If deployment cannot preserve those facts, stop before restart and report the
failed precondition. Do not hand-edit the row or fall back to changing compiled
defaults.

## Open questions

None. The supported activation path, precedence, live stored document, current
effective document, failure behavior and identity-preservation boundary are all
evidenced by the task, approved `OG-054`, repository code and live readback.

## Handoff

Implement only this contract in the already-bound `implement` AgentRun
`01a0b033-9bec-7b83-8eff-ce5d414b8434`. Preserve this TeamRun and worktree,
record the `high-change` plus mutation evidence, then hand the exact candidate
to the original `verify` slot. Scope does not authorize merge, gate recording,
Jira transition or deployment before independent verification.
