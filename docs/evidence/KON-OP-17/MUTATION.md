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
  request scope, with static fleet entries limited to compatibility overrides.

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
```

The full format, lint, Rust workspace, generated-contract, console, and release
build gates are recorded in the associated deployment report and commit/PR.
