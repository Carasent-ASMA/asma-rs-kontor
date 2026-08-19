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

## Restoration receipt

No mutant remains:

- legacy baseline uses `seats: Vec::new()`;
- duplicate TeamRun and AgentRun cardinalities are both checked;
- the current epic revision must equal `expected_revision`;
- a non-replayed recovery requires `agent.binding.is_none()`.

After restoration:

```text
test a_legacy_epic_bootstraps_one_frozen_roster_and_one_leadership_pair ... ok
test result: ok. 1 passed; 0 failed; 180 filtered out

test exact_resume_recovers_one_durable_admission_without_the_scheduler_key ... ok
test result: ok. 1 passed; 0 failed; 180 filtered out
```

The full format, lint, Rust workspace, generated-contract, console, and release
build gates are recorded in the associated deployment report and commit/PR.
