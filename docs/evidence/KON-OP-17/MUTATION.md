# KON-OP-17 / ASMA-7950 — recovery-surface mutation proof

Date: 2026-08-20
Branch: `fix/ASMA-7950-legacy-roster-exact-admission-recovery`
Requirement: OP-REQ-030 operational-gap closure for legacy roster bootstrap and
exact queued-admission recovery.

## What this proves

Each mutation below was applied to the corrected source, the named focused test
was run against that broken build, and the mutation was immediately restored.
Both focused tests were then rerun green on the restored tree.

| # | Deliberate mutation | Killer test | Observed failure |
| --- | --- | --- | --- |
| M1 | Treat the published Core Team itself as the legacy epic's baseline instead of an empty roster | `a_legacy_epic_bootstraps_one_frozen_roster_and_one_leadership_pair` | killed: preview returned `effects: []`; assertion reported missing LSA `seat_created` effect |
| M2 | Disable duplicate TeamRun/AgentRun detection in `scheduler:resume` | `exact_resume_recovers_one_durable_admission_without_the_scheduler_key` | killed: duplicate request reached the runtime and returned 422 instead of the required pre-runtime 400 |
| M3 | Disable the epic revision fence in `scheduler:resume` | same exact-resume test | killed: stale-revision request reached the runtime and returned 422 instead of 409 |
| M4 | Allow a fresh recovery key to address an already-bound AgentRun | same exact-resume test | killed: second fresh resume returned 200 with the preserved seats instead of 409 |
| M5 | Parse `runtimes.json.mini_project_id` as a generic `ExternalId` again | `a_paseo_plane_refuses_a_jira_key_as_its_kontor_epic_identity` | killed: the refusal moved past the fleet-field boundary and surfaced only as the adapter's generic `execution plane` error |
| M6 | Let a directly embedded Paseo adapter accept any `ExternalId` as its configured epic selector | `a_direct_adapter_refuses_a_non_kontor_epic_selector` | killed: the adapter composed successfully with `ASMA-7869` instead of refusing before runtime use |
| M7 | Skip native-container preparation from `topology:materialize` | `materializing_a_ticket_binds_its_native_workspace_without_admitting_a_run` | killed: HTTP 200 returned a TSW whose `observed_binding` was still null |
| M8 | Look up a materialization replay against the project aggregate instead of the epic aggregate recorded by its receipt | same materialization test | killed: the same-key replay returned HTTP 409 `idempotency_conflict` instead of the original receipt and native workspace |
| M9 | Restore the native-child title renderer's static `task_scopes` lookup instead of using the request's durable `ExecutionScope` | `a_dynamic_task_uses_its_durable_scope_without_a_static_task_entry` | killed: placement returned `WorkspaceMismatch` / `the task has no configured Paseo workspace scope`, reproducing the live OP-18 refusal |
| M10 | Suppress the transactional `unbound` → `bound` node update after the exact native container is persisted | `materializing_a_ticket_binds_its_native_workspace_without_admitting_a_run` | killed: the projection returned a concrete native id/cwd while still claiming `placement=unbound`, reproducing the contradictory OP-18 readback |
| M11 | Remove managed-checkout preparation from the native-child materialization path | `ticket_materialization_creates_the_absent_checkout_before_workspace_registration` | killed: the mocked Paseo registration returned, but the asserted linked Git worktree did not exist |
| M12 | Create a new task branch from the control plane's current `HEAD` instead of the repository default branch | `preparation_creates_an_absent_declared_git_worktree_before_registering_it` | killed: the prepared branch inherited the deliberately divergent in-flight control-plane commit instead of `master` |
| M13 | Suppress the exact branch check for an existing managed worktree | `preparation_refuses_branch_drift_before_registering_a_workspace` | killed: the expected typed checkout refusal disappeared and execution advanced to a later workspace mismatch |
| M14 | Classify checkout-preparation failure as runtime `unavailable` again | `checkout_preparation_is_a_typed_placement_block_not_a_runtime_outage` | killed: the API returned `Unavailable` instead of `PlacementBlocked` |

## Restoration receipt

No mutant remains:

- legacy baseline uses `seats: Vec::new()`;
- duplicate TeamRun and AgentRun cardinalities are both checked;
- the current epic revision must equal `expected_revision`;
- a non-replayed recovery requires `agent.binding.is_none()`;
- the fleet loader parses the configured selector as `MiniProjectId` before it
  constructs an adapter;
- direct adapter construction enforces the same typed selector invariant.
- a non-logical materialization prepares and persists the runtime-issued native
  container before it returns;
- materialization replay lookup and receipt storage both address the owning
  epic aggregate;
- native-child placement and retitle render task identity from the durable
  request scope, with static fleet entries limited to compatibility overrides;
- a persisted exact native container and the node's `bound` placement commit in
  the same transaction, with replay leaving the node revision stable.
- native-child materialization and legacy workspace preparation both create an
  absent managed canonical checkout before Paseo workspace registration;
- a new task branch starts from `origin/HEAD`, then local `master`/`main`, never
  from the daemon's current in-flight branch;
- an existing managed checkout must belong to the same Git common directory
  and carry the exact branch encoded by its canonical path;
- a checkout precondition or conflict is `placement_blocked`, not a fabricated
  runtime outage.

After restoration:

```text
test a_legacy_epic_bootstraps_one_frozen_roster_and_one_leadership_pair ... ok
test result: ok. 1 passed; 0 failed; 180 filtered out

test exact_resume_recovers_one_durable_admission_without_the_scheduler_key ... ok
test result: ok. 1 passed; 0 failed; 180 filtered out

test a_paseo_plane_refuses_a_jira_key_as_its_kontor_epic_identity ... ok
test a_direct_adapter_refuses_a_non_kontor_epic_selector ... ok
test materializing_a_ticket_binds_its_native_workspace_without_admitting_a_run ... ok
test a_dynamic_task_uses_its_durable_scope_without_a_static_task_entry ... ok
test ticket_materialization_creates_the_absent_checkout_before_workspace_registration ... ok
test preparation_creates_an_absent_declared_git_worktree_before_registering_it ... ok
test preparation_refuses_branch_drift_before_registering_a_workspace ... ok
test checkout_preparation_is_a_typed_placement_block_not_a_runtime_outage ... ok
```

The full format, lint, Rust workspace, generated-contract, console, and release
build gates are recorded in the associated deployment report and commit/PR.
