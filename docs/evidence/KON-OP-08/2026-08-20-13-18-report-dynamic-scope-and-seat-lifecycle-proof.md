# OP-08 schema-46 reconciliation, lifecycle, and multi-hop proof

Date: 2026-08-20
Task: `01a0074f-672e-79a3-9876-d0e1bf585d4e` / ASMA-7877, revision 2
TeamRun: `01a0195b-7280-7500-81cf-c28023f8cbf8`
Predecessor AgentRun: `01a0195b-a597-7c50-9c17-1d36fa535aaa`
Inspector AgentRun: `01a01ead-7837-7bf1-b63b-cd596c9b0d97`
Integrated base: `6724ad60ac991a4de9087c53eb6de4d8e44e0ade`
Green code checkpoint: `29e6636` (`fix(kontor): Reconcile OP-08 with schema 46 ASMA-7877`)
Status: local candidate green; push, PR CI, Kontor settlement, and inspector verdict are separate follow-up receipts

## Reconciliation result

The preserved branch was replayed onto the schema-46 platform line with an
explicit OP-08 allowlist. The resulting lineage retained the OP-08 architecture,
Jira multi-hop policy and control-surface behavior, and the staged lifecycle and
materialization slice. Obsolete OP-07 integration ancestry and OP-18-owned
project read/list work (`fd0a61d`) were dropped.

The schema-46 `ExecutionScope` remains the single durable scope model. No
parallel `TaskExecutionScope`, startup-inventory authority, Task schema change,
store migration, or generated contract change was introduced. The current
durable task `short_code`, Jira ticket link, epic relation, worktree, and runtime
selection are resolved before native effects.

The retained OP-08 behavior is:

- a requested Jira milestone may converge through exactly the intermediate
  status declared by the pinned workflow specification;
- the request, live-transition check, lost-ack reconciliation, and receipt all
  name the status reached by the current hop rather than the later milestone;
- ticket materialization creates the durable ECP/TSW node chain and binds the
  canonical native TSW, but creates no TeamRun and no delivery seat;
- scheduler start owns delivery-seat admission, and an exact replay reuses the
  same TeamRun, AgentRuns, bindings, and native identities;
- `live_seat` excludes terminal TeamRuns and excludes a terminal AgentRun from
  an otherwise-open TeamRun;
- terminal or abandoned TeamRuns release their active topology seat bindings,
  preserving the rows as history while freeing the active node/slot key; and
- an active seat is reusable only for the same task and exact TeamRun. A live
  binding owned by another task or generation is a typed placement refusal.

## Behavior proof

`an_applied_task_materializes_and_replays_without_a_startup_task_scope` starts
with an empty daemon/runtime task inventory, applies OP-08 with canonical short
code `OP-08`, Jira issue `ASMA-7877`, and worktree `/w/op-08`, then executes:

`apply -> topology:materialize -> execution:arm -> scheduler:plan -> scheduler:start -> exact replay`

It proves the native TSW is titled `TSW · ASMA-7877 · OP-08`, is rooted at the
applied worktree, and has no delivery seats after materialization. Scheduling
then creates one TeamRun and its declared seats; replay returns the same
TeamRun, AgentRun, and native ids with `applied: unchanged`.

The lifecycle tests prove that an abandoned or closed TeamRun cannot receive a
new context snapshot, a terminal AgentRun cannot receive one while a sibling
seat remains live, released topology bindings no longer hold active slots, and
a later admitted generation receives fresh bindings on the same TSW.

The Jira policy and ASMA boundary suites prove a two-hop target uses only the
pinned intermediate, refuses an invented path, declares the current hop in the
outbound request, and confirms a lost acknowledgement only against the status
that hop was meant to reach.

## Mutation proof

Each defect was applied alone with `apply_patch`, its owning test was run to the
expected red result, and the exact source hunk was restored before the next
mutation.

| Deliberate defect | Killing test | Expected red observation |
| --- | --- | --- |
| include terminal TeamRuns in `live_seat` | `a_team_closes_on_settled_turns_while_every_seat_stays_live` | context returned 200 instead of typed 422 |
| allow a terminal AgentRun to own new context | `a_terminal_agent_run_is_not_the_live_seat_of_its_still_open_team` | terminal run id was selected |
| suppress terminal TeamRun seat release | `a_team_closes_on_settled_turns_while_every_seat_stays_live` | four delivery bindings remained active |
| materialize a ticket without its ECP node | `an_applied_task_materializes_and_replays_without_a_startup_task_scope` | scheduler start was placement-blocked because the delivery seats had no control plane |
| create a control/delivery seat on the ticket during materialization | same tracer | materialized task node was not seat-empty |
| refuse exact-TeamRun reuse of its own live binding | same tracer | replay returned zero seats with typed `placement_blocked` |
| disable the workflow-declared intermediate hop | `a_target_two_moves_away_is_reached_through_the_declared_intermediate` | policy returned `NoLiveTransition` |
| declare the final milestone instead of the current hop | `a_staged_hop_request_declares_the_hop_not_the_milestone` | request named status `10214` instead of hop `10213` |

The restored candidate then passed the complete pinned CI-equivalent gates.

## Final gates

| Gate | Result |
| --- | --- |
| `git diff --check` | pass |
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass, no warnings |
| `cargo test --workspace` | pass; loopback API 192/192, Paseo contract 136/136, all other workspace and doc tests green; live-runtime-only tests remained explicitly ignored |
| `cargo audit` | pass; 19 repository-allowed warnings, no failing advisory |
| `cargo deny check` | pass: advisories, bans, licenses, and sources all ok |
| `pnpm install --frozen-lockfile` | pass with pnpm 11.6.0 |
| `pnpm --filter kontor-console verify:api` | pass; generated types equal the committed OpenAPI contract |
| `pnpm -r typecheck` | pass |
| `pnpm -r test` | pass: 16 files, 295 tests |
| `pnpm audit --prod` | pass: no known vulnerabilities |

The first Rust build attempt could not resolve GitHub while fetching the exact
locked Swagger UI artifact. The approved network rerun downloaded it and
completed every Rust gate. The first pnpm audit attempt likewise reached the
sandbox DNS boundary; the approved rerun returned no known vulnerabilities.

## Boundaries and preserved evidence

- OP-18's project read/list, workflow-spec installation semantics, and task
  withdrawal implementations are consumed from the integrated master line and
  are not duplicated by this branch.
- OP-14's serialized task-state/schema lane is not modified or claimed. This
  slice adds no migration of any number.
- Canonical short-code naming, provider-outage enforcement, hosted-seat
  retirement, retitle behavior, and Core Team semantics from PRs 49-63 are
  preserved and covered by the full workspace suite.
- The two predecessor KON-MVP-18 evidence directories
  `run-0f10b0648fb1e0be/` and `run-70586acfdf538eb4/` were never staged, reset,
  cleaned, deleted, or read into this change.
- The full workspace pilot generated a third untracked evidence directory,
  `run-855d4d03953de182/`. It is intentionally left untracked and unmodified
  after generation; no KON-MVP-18 evidence is part of OP-08's diff.
- No seat or topology was created or replaced while producing this candidate.
  The existing inspector must record the independent gate.

The existing inspector should review the branch/PR candidate against task
revision 2, this report, the CI receipts, and the exact OP-08 commit range. The
author must not self-record the inspector verdict.
