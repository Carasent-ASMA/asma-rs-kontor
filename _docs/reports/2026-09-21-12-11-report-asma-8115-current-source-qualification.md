# ASMA-8115 Current-Source Qualification

> **Date:** 2026-09-21 12:11
> **Status:** 🟡 In Review
> **Category:** report
> **Scope:** PUB-07 scheduler placement admission, exact native container readback, and readback-only implement-seat succession in `asma-rs-kontor`
> **Summary:** Qualifies exact current source `44663e10` against the ASMA-8115 PUB-07 scope and records the live Kontor task/run/container provenance used for the implement-to-verifier handoff. No uncovered scoped defect was found and no production source, deployment, merge, or runtime topology was changed.

---

## When to Load

**Load this document when:**

- independently verifying the ASMA-8115 `high-change` artifact;
- auditing scheduler admission against exact Jira, Team Definition, ESW/ECP/TSW, worktree, or native readback facts;
- checking the PUB-07 readback-only succession and TSW workspace-kind fences on source `44663e10`.

**Do NOT load for:** deployment, merge, release ownership, or unrelated scheduler and container-recovery work.

---

Artifact: `high-change`

## Authoritative scope and control-plane state

The authoritative project plan is `plan/infrastructure-publication-identity-enforcement-1.md`, sections 2.10 and 2.11. PUB-07 requires scheduler admission to fail closed until the exact confirmed Jira binding, pinned Team Definition rendering, and ESW/ECP/TSW bindings and native readbacks are proven.

Kontor was read directly before qualification:

| Field | Observed value |
| --- | --- |
| Project | `01a0064a-e056-7603-9968-ef64fdaacb75` |
| Epic | Jira `ASMA-8101`; Kontor `01a0721b-ea30-7fe3-88a5-4d33ca613414` |
| Task | Jira `ASMA-8115`; Kontor `01a075dc-74e4-71f0-9e63-775bd79391c9`; revision `1` |
| Jira binding | `confirmed`, revision `1`, readback hash `b25f25ad530514d41c7c7822a93f067d1911e9f6b7c56e2a054646f751c4f026` |
| Workflow | `asma-high-stakes-primary-20260829@1`; phase `high-implementation` |
| Required next artifact | `high-change`, handed from `implement` to `verify` |
| Verification gate | `high-verification-gate`, evaluator `fleet-verifier`, requires `high-change` plus `high-verification-report` |
| TeamRun | `01a07636-ce37-7b61-91a8-8e6faf7e01eb`, lifecycle `running` |
| Current implement AgentRun | `01a0c348-e77d-7470-be3d-7415116b7abb`, revision `5`, parent `01a07637-5060-72e0-85f3-f162d3c47def` |
| Current native seat | `4247e5db-25d2-435c-b851-f32e21ee99a3`, attached generation `1`, `paseo.agent` |
| Last Kontor runtime observation | cursor `4568`, reachable/running at `2026-09-21T10:01:30.584308Z` |
| Restored TSW workspace | `wks_7d16eeb2a644d90f`; exact task worktree path; native kind read back as `worktree` in approved recovery evidence |

The approved memory items `asma-8115-readback-only-succession-fix-complete-20260919` and `operational-gap-asma-8115-orphan-predecessor-20260920` were used only for their stated historical scope, identity, recovery, and filesystem receipts. The current implement AgentRun and native seat were independently read back from Kontor.

## Exact source provenance

| Field | Value |
| --- | --- |
| Qualified source | `44663e10e063ad3d11c0c84ca6a87d680d75e835` |
| Source subject | `ASMA-8203 Recover uncertain delivery and scoped leadership tools (#264)` |
| Source tree | `c5ea3516f75d2144e2c33224d016d877f602207c` |
| Qualification worktree | `/Users/igor/carasent/asma-rs-kontor.worktrees/asma-8115-current-source-qualification`, created at exact `44663e10`, branch `docs/ASMA-8115-current-source-qualification` |
| Original task module checkout | preserved at `7ee46b01fafcc9458d7e1e894f8277ada7912742` |

The historical feature hashes `f03b8827` and `c594c150` are not literal ancestors of `44663e10` after the coherent-candidate integration history. Qualification therefore does not infer delivery from hash ancestry. It reads and tests the current source directly. `git log -S` places both the current `placement_admission` implementation and the `ControlPlaneObservation::drivable` field on mainline at `e3a0c6c866b8e5e8949ea36e2a223ba88feb7cf6`; the task-only TSW-kind regression entered mainline at `570b29a46030e006d263c720dbe9b74ff17122e3`, and the post-inspection placement-cache fence at `0fabd9ce4b8c736bb815585b855388fef7f6f6c4`.

Representative qualified blobs at `44663e10`:

| Path | SHA-256 |
| --- | --- |
| `crates/kontor-daemon/src/applications.rs` | `a5d6c3635f93e244720ac12b664053ba210d8bcb9eaf26cdfc20199dd0b87e66` |
| `crates/kontor-runtime-paseo/src/adapter.rs` | `6166b2e8e2ceb5cf2443eb04930b0a4437e18e68b6ddb89483c2da3c9a3ec412` |
| `crates/kontor-runtime/src/container.rs` | `e3b1435b5212d1070655fa71a3cc3947a93036563762f2a616f44c542dd12161` |
| `crates/kontor-runtime/src/observation.rs` | `aa726d181cc8c323026f249c63ab22f21cb61dae85ae330bcf666fdfb5441442` |

## Scope-to-source assessment

No uncovered scoped defect was found.

1. `Services::placement_admission` reads one confirmed epic Jira key and one confirmed task Jira key, validates both, loads the exact immutable Team Definition pin and revision, and refuses a pin/revision mismatch.
2. It rejects undeclared delivery slots, missing or unverified task worktrees, and non-canonical worktree paths before admission.
3. It requires exactly one active ESW, ECP, and task TSW with the required parent relationships and pinned-definition container declarations.
4. For each required container it performs an exact runtime inspection and compares durable/native identity, parent, projection, topology correlation, rendered title, canonical CWD, and the task TSW CWD against the verified task worktree.
5. Only the confirmed path emits a canonical placement-attestation digest. `Services::snapshot` places that result on the scheduler candidate; `Services::start` recomputes the snapshot and plan and requires the exact plan hash before recording the start command or admitting seats.
6. `PaseoAdapter::inspect_container` reads projects and workspaces by exact native ID. For a task-scoped child it maps the runtime workspace kind through `ContainerWorkspaceKind::is_applicable_to(true)`, under which only `Worktree` is accepted; `LocalCheckout`, `Checkout`, `Directory`, and unknown kinds fail closed.
7. `ControlPlaneObservation::drivable` preserves the readback-only succession distinction: a reachable but non-drivable predecessor follows the existing linked-successor path instead of being reused.

The full current-source knowledge-graph index recorded no coverage issue for the six production files and four focused test files used in this assessment. This remains best-effort graph metadata; the named source was also read directly and the behavior was executed through focused tests.

## Current-source verification

All effective focused commands ran in the exact-source worktree at `44663e10`:

| Command target | Result |
| --- | --- |
| `kontor-runtime` / `the_fake_refuses_a_ticket_container_that_is_not_a_worktree` | PASS, 1/1 |
| `kontor-runtime-paseo` / `a_bound_tsw_container_refuses_every_non_worktree_shape` | PASS, 1/1 |
| `kontor-runtime-paseo` / `continuity_an_observation_states_whether_the_seat_can_be_driven` | PASS, 1/1 |
| `kontor-scheduler` / `every_unconfirmed_placement_fact_refuses_with_its_exact_reason` | PASS, 1/1 |
| `kontor-daemon` / `scheduler_planning_only_inspects_exact_materialized_topology_and_preserves_its_digest` | PASS, 1/1 |
| `kontor-daemon` / `scheduler_launch_refuses_when_the_materialized_plane_disappears` | PASS, 1/1 |
| `kontor-daemon` / `explicit_materialization_places_an_unconfigured_project_before_scheduler_launch` | PASS, 1/1 |

An initial invocation looked for `every_unconfirmed_placement_fact_refuses_with_its_exact_reason` in the daemon loopback target and executed zero tests. It is excluded from evidence. The corrected scheduler-target invocation above executed the real test and passed.

Historical mutation evidence remains in `docs/evidence/ASMA-8115/MUTATION.md`; no new mutant was introduced during this current-source qualification.

## Boundaries and handoff

- No production source defect was found, so no production source was changed.
- No deployment, merge, PR, watchdog, scheduler start/resume, seat replacement, container mutation, or topology mutation was performed.
- The original TeamRun, AgentRun lineage, task worktree, restored TSW workspace, and current native seat identities were preserved.
- QA must independently verify exact source `44663e10`, this report, and the registered `high-change` locator. QA owns the `high-verification-report` and `high-verification-gate` verdict; this implement seat does not self-approve either.

There are no unresolved scope ambiguities or open questions from this qualification.
