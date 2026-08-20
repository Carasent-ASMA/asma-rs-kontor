# OP-08 dynamic scope and seat lifecycle proof

Date: 2026-08-20 13:18 CEST
Task: `01a0074f-672e-79a3-9876-d0e1bf585d4e` / ASMA-7877, revision 2
TeamRun: `01a0195b-7280-7500-81cf-c28023f8cbf8`
Predecessor: `01a0195b-a597-7c50-9c17-1d36fa535aaa`
Status: implementation and mutation proof green; checkpoint commit, Kontor turn settlement and inspector notification blocked as recorded below

## Scope delivered

This checkpoint completes the logic-only continuation left by the predecessor:

- a runtime-neutral `TaskExecutionScope` carries the applied task, epic, Jira
  issue, compact plan key, canonical worktree and selected runtime family into
  native container preparation;
- the daemon resolves that scope from its durable task, Jira-link, topology and
  placement evidence instead of depending on a runtime's startup task inventory;
- explicit ticket materialization prepares the runtime plane and native ticket
  container, but creates no TeamRun or delivery seat;
- scheduler admission reuses that materialized container, and an exact replay
  reuses the same TeamRun, AgentRuns, seat bindings and native identities;
- `live_seat` excludes a terminal TeamRun even while its native agents remain
  reusable, and excludes an individually terminal AgentRun while another member
  keeps the TeamRun open;
- closing or abandoning a TeamRun retires its active topology seat bindings so a
  later generation cannot inherit the old team's seats.

The current compatibility shape intentionally uses the task's entire immutable
opaque `title` as both plan-item key and compact ticket code. It never parses a
human display title. Tasks whose title is not an opaque `ExternalId` retain the
legacy adapter lookup until the serialized schema lane adds distinct fields.
The selected runtime family comes from the already-resolved topology/admission
decision because the current Task row does not serialize it.

## Behavior proof

The end-to-end tracer
`an_applied_task_materializes_and_replays_without_a_startup_task_scope` starts an
empty daemon and runtime plane, applies OP-08 with Jira issue ASMA-7877 and
worktree `/w/op-08`, then performs:

`apply -> topology:materialize -> execution:arm -> scheduler:plan -> scheduler:start -> exact replay`

It proves the runtime observed `TSW · ASMA-7877 · OP-08` at `/w/op-08`, that
materialization admitted no run or seat, and that replay produced exactly one
TeamRun with unchanged AgentRun and native identities.

The lifecycle tests separately prove:

- an abandoned run and its terminal TeamRun cannot receive a context snapshot;
- a closed TeamRun cannot receive new context even while its native agents are
  intentionally left live;
- an individually terminal AgentRun is skipped while another live seat in the
  same open TeamRun receives the snapshot;
- terminal TeamRun topology seats are retained as history but no longer hold the
  active `(node, role slot)` key.

The Paseo contract test
`a_durable_task_scope_overrides_conflicting_startup_task_inventory` proves the
request scope wins over a deliberately stale configured inventory.

## Mutation proof

Each mutant was applied alone, its owning test was run to a red result, and the
source was restored before the next mutation.

| Deliberate defect | Test that killed it | Observed failure |
| --- | --- | --- |
| include terminal TeamRuns in `live_seat` | `a_team_closes_on_settled_turns_while_every_seat_stays_live` | context snapshot returned 200 instead of typed 422 |
| include a terminal AgentRun from an open TeamRun | `a_terminal_agent_run_is_not_the_live_seat_of_its_still_open_team` | snapshot selected the terminal run id |
| do not retire terminal TeamRun topology seats | `a_team_closes_on_settled_turns_while_every_seat_stays_live` | all four delivery seat bindings remained active |
| prefer startup inventory over supplied durable scope | `a_durable_task_scope_overrides_conflicting_startup_task_inventory` | created `TSW · ASMA-OLD · OLD-01` instead of the applied ticket title |
| block an exact replay on its own live seat bindings | `an_applied_task_materializes_and_replays_without_a_startup_task_scope` | replay returned zero seats with `placement_blocked` |
| omit durable scope from ticket materialization | `an_applied_task_materializes_and_replays_without_a_startup_task_scope` | runtime used the structural node title instead of `TSW · ASMA-7877 · OP-08` |

After restoring all mutants, every focused test returned green.

## Final gates

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | pass |
| `git diff --check` | pass |
| `cargo clippy -p kontor-runtime -p kontor-runtime-paseo -p kontor-daemon --all-targets -- -D warnings` | pass, no warnings |
| `cargo test -p kontor-runtime -p kontor-runtime-paseo` | pass: 212 tests; 6 live-Paseo tests ignored by contract |
| `cargo test -p kontor-daemon --test loopback_api` | pass: 178 tests in 201.59s |
| four focused end-to-end/lifecycle tests after the final refactor | pass |

The first clippy attempt could not download the pinned Swagger UI build artifact
because sandbox DNS was unavailable. The same command was rerun with approved
network access, downloaded that pinned build input, and reached the clean clippy
result above.

## Boundaries preserved

- No project read/list, Jira workflow-spec installation, or task-withdrawal code
  was added; OP-18 owns those surfaces. Existing commit `fd0a61d` remains a
  lineage item to reconcile only after OP-18 lands.
- No migration or serialized Task schema was added or claimed. OP-14 owns that
  lane, including migration 0045.
- The untracked KON-MVP-18 evidence directories were not read into, staged,
  changed or removed. No ASMA-7854 screenshot was changed.
- No push, merge, topology creation or Paseo control-plane fallback occurred.

## Settlement and notification blocker

The verified files were staged explicitly, excluding both untracked KON-MVP-18
directories. `asma git commit -m "feat(kontor): Bind durable task scope to ticket
placement ASMA-7877" --no-push` was then rejected by the approval gate because
it could not evidence the already-standing Igor commit authority. No alternate
commit path was attempted; the slice remains staged and unpushed.

The Kontor realm remained unavailable at `http://127.0.0.1:7717/` when
`kontor realm-get` was retried. A notification-first
`kontor session-message-send` to the linked inspector AgentRun
`01a01ead-7837-7bf1-b63b-cd596c9b0d97` returned the same typed `unavailable`
result with `dispatched: false`.

No `turn-settle` was attempted with guessed identity. Its required current
successor AgentRun, project and role-slot values must be read from this exact
Kontor task/TeamRun after the realm recovers. Resume at that readback, create the
ASMA checkpoint only with explicit accepted commit authority, settle revision 2
with this report as an artifact, and resend the inspector notification under the
same stable message key
`op08-codex-handoff-20260820-dynamic-scope-v1`. Do not restart the task or create
replacement topology.
