# KON-OP-08 / ASMA-7877 — independent corrective re-review

> **Date:** 2026-08-20 22:44 CEST
> **Status:** 🔴 Rejected
> **Author:** Inspector · KON-OP-08
> **Category:** report
> **Scope:** task revision 2, TeamRun `01a0195b-7280-7500-81cf-c28023f8cbf8`
> **Summary:** Independent re-review of corrective commit `01ca724d` and PR 64 merge head `4e3e8fd`. The topology replay repair and run-projection repair are accepted as working slices, but two original task-acceptance blockers remain and one claimed mutation protection is not pinned by the committed tests.

---

## When to Load

**Load this document when:**

- correcting or re-reviewing OP-08 revision 2 after head `4e3e8fd`;
- reconciling the OP-08 code-review gate or its outstanding evidence;
- changing provider-outage seat retirement, runtime observation reduction, Jira
  reconciliation, or the remaining OP-08 compatibility scope.

**Do NOT load for:** approval of PR 64, unrelated Kontor work, or Jira-state
reconciliation.

---

## Verdict

PR 64 at `4e3e8fd774c04c6bd95bcd99f66b143df615b142` is **rejected**.
Do not merge it on this inspection record.

The corrective commits repair meaningful defects. In particular, prior
FND-003 is resolved: materialization now re-ensures the logical ticket chain on
idempotent replay and does not return an existing task node before ensuring its
ECP. The new committed regression reproduces the legacy missing-ECP shape and
preserves the exact TSW/native binding.

That correction does not resolve the two task-level blockers below. The
provider-outage claim also has a survived mutant, contradicting the handoff's
unqualified statement that all three behavioral mutants were killed.

## Immutable inspection identity

| Item | Value |
| --- | --- |
| Realm | `01a00649-9ee6-73e0-ba1b-6a6c35cfd065` |
| Project | `01a0064a-e056-7603-9968-ef64fdaacb75` |
| Epic | `01a0074f-6719-7570-adf7-95ee3ec69875` |
| Task | `01a0074f-672e-79a3-9876-d0e1bf585d4e`, revision 2 |
| TeamRun | `01a0195b-7280-7500-81cf-c28023f8cbf8` |
| Builder turn receipt | `01a020e3-015d-7ea2-9f1a-0f2d5fb851ba` |
| Builder AgentRun | `01a01eb0-453d-7c21-ac60-bee1d8cf8d73` |
| Inspector AgentRun | `01a01ead-7837-7bf1-b63b-cd596c9b0d97` |
| Corrective commit | `01ca724d7deb45e0241e140cebef3ad4afc9fd82` |
| PR merge head | `4e3e8fd774c04c6bd95bcd99f66b143df615b142` |
| Current PR base | `6868a1414bc44adc0eb0813ced8943c8f41734b2` |
| Pull request | [PR 64](https://github.com/Carasent-ASMA/asma-rs-kontor/pull/64) |

Live Kontor readback at snapshot cursor 380 preserves the exact builder and
inspector bindings/native ids. Both runs read `desired=run_requested`,
`observed=running`, `derived=confirmed`, `lifecycle=running`; neither run is
terminal. The TeamRun is running. The task remains in `implementation` and the
declared code-review gate remains `not_ready`.

The handed QNR cursor-374 readback was not needed to reach this verdict. Its
identity-preservation claims are consistent with the owned repair, but no
project/epic identity was supplied for an independent refetch in this review.

## Blocking findings

### RR-FND-001 — Jira apply still reports a staged hop as final convergence

Severity: **blocking; unresolved from the first inspection**.

The approved architecture requires a fresh readback with a closed
`converged`, `progressed`, or `unconfirmed` disposition and specifically
forbids reporting an attempted intermediate hop through a non-empty collection
named `converged`.

The merged head still returns all planned links in `converged` after applying
one transition (`crates/kontor-daemon/src/applications.rs`, lines
14394-14533). Neither `TicketDelegation::apply` nor
`StatusTransitionReceipt::validate` verifies that the refetched observation is
the declared intermediate or final milestone. No committed test exercises the
public apply response for the required two-hop sequence or rejects the named
false-success receipt.

The corrective runtime-projection work does not touch this path. A live first
hop can therefore still be described as final convergence before Jira proves
the final milestone.

Required correction: implement and persist the closed post-effect disposition,
validate the exact readback destination, and commit the full staged-hop,
lost-acknowledgement, contradictory-readback and false-success receipt
regressions required by the approved architecture.

### RR-FND-002 — most of the approved OP-08 control-surface scope remains absent

Severity: **blocking; unresolved from the first inspection**.

No approved task, architecture, or current Kontor memory record supersedes the
canonical OP-08 goal. `memory-search` reports no approved current project-memory
revisions. The central plan still owns CLI/MCP parity, bootstrap/client
registration, ASMA compatibility forwarders, native connector cutover and
subprocess removal.

Concrete contradictions remain in the merged head:

- `Services::jira_specs` still selects
  `catalog.field_specs().first()` and
  `catalog.workflow_specs().first()` at lines 1496 and 1511 instead of an exact
  installed project pair pin.
- CLI has workflow-spec install but no field-spec install and no atomic
  compatible field/workflow pair-pin operation/readback.
- `AsmaExecutable` and `kontor-integrations-asma` remain production dependencies
  and the Jira reconcile path still invokes them.
- The candidate range changes no Kontor CLI, MCP, bootstrap, client-registration
  or ASMA compatibility-forwarder implementation files.

The current approved plan's acceptance still requires full `/v1`/CLI/MCP
semantic parity, agent-runnable bootstrap, fixed forwarders, zero ASMA runtime
edges and an `asma`-absent primary journey. Passing a smaller runtime-lifecycle
repair cannot satisfy those unimplemented criteria.

Required correction: implement the still-approved task, or commit an explicit
authorized supersession that accounts for every removed requirement before
presenting another task-level gate candidate.

### RR-FND-003 — progressed-seat outage-retirement guard has a survived mutant

Severity: **blocking test-evidence gap in the corrective repair**.

The production guard at `crates/kontor-daemon/src/applications.rs`, lines
16741-16751 correctly limits provider-outage retirement to a run whose durable
lifecycle, desired state and observed state all describe launch-only evidence.
However, the only daemon test that sends `unavailable_provider`,
`an_admin_retires_an_exact_never_dispatched_provider_blocked_seat`, exercises
only the allowed launch-only case.

Independent mutation pass:

| Mutant | Suite | Result |
| --- | --- | --- |
| Delete the complete launch-only lifecycle/desired/observed guard | `cargo test -p kontor-daemon --test loopback_api an_admin_retires_an_exact_never_dispatched_provider_blocked_seat -- --exact` | **SURVIVED** — test remained green |

The original source was restored immediately. Its Git blob before and after was
`144b6ac44993f755c90522dad5df5b985c63e31d`; the candidate diff and status were
unchanged. Repository search finds no second daemon request fixture carrying
`unavailable_provider`, so no committed behavior test attempts retirement after
`running` or `waiting_input` evidence.

Required correction: strengthen the existing test so the same run first
progresses beyond launch, then prove exact provider-outage replacement is
refused before runtime retirement. Re-run the deletion mutant and record the
specific failing assertion.

## Accepted corrective evidence

The following portions of the repair are independently green:

- `replaying_ticket_materialization_repairs_a_missing_epic_control_plane`;
- `a_run_the_runtime_says_is_still_working_is_not_settled`;
- `exact_resume_recovers_one_durable_admission_without_the_scheduler_key`;
- `fresh_runtime_evidence_converges_non_terminal_lifecycle_without_regression`;
- `a_raw_event_is_appended_before_state_is_reduced_and_replays_are_idempotent`;
- `an_admin_retires_an_exact_never_dispatched_provider_blocked_seat` for its
  positive launch-only case;
- `cargo fmt --all -- --check`.

The handed local clippy, workspace, cargo-deny and other focused results are
consistent with the candidate. Final GitHub readback confirms both hosted
Console gates and both hosted Rust workspace gates completed successfully at
the exact head and base recorded above. PR 64 remains open and unmerged. These
green gates do not resolve the three blocking findings.

`git diff --check 6868a141..4e3e8fd` still exits 2 on trailing whitespace in
the two committed OP-08 metadata blocks. This is an evidence-hygiene issue, not
one of the three blocking behavioral/scope findings.

Jira reconciliation remains unavailable: `asma jira sync --ticket ASMA-7877
--dry-run` returns `JIRA_BASE_URL is not set`. No Jira state was inferred.

## Open-question ledger

### OQ-001 — attributable evaluator account remains unresolved

- **Subject:** the account profile authorized to record the inspector verdict.
- **Attached record:** this report; task
  `01a0074f-672e-79a3-9876-d0e1bf585d4e`; TeamRun
  `01a0195b-7280-7500-81cf-c28023f8cbf8`; Inspector AgentRun
  `01a01ead-7837-7bf1-b63b-cd596c9b0d97`.
- **Why ambiguous:** current supported readback still reports
  `account_profile_id: null` for the inspector, while `kontor gate-record`
  requires `--evaluator-account`. The gate is also `not_ready`.
- **Options observed:** explicitly attach and read back the correct account on
  this same seat; assign an already-authorized account through a supported
  Kontor operation; or repair the accountless-successor/gate-record contract.
- **Disposition:** unresolved. No account was inferred or borrowed.

## Recording checkpoint

The intended typed operation is `code-review-gate = rejected` for task revision
2, citing head `4e3e8fd` and this review. It cannot be safely formed while
OQ-001 is unresolved and cannot be accepted while the gate is `not_ready`.

The durable fallback is
[PR comment 5361476086](https://github.com/Carasent-ASMA/asma-rs-kontor/pull/64#issuecomment-5361476086),
which contains this complete report. It changes no code, Kontor/Paseo/Jira
state, topology, candidate head or merge state. The owning TPM/control-plane
owner must settle gate readiness and evaluator identity through supported
Kontor surfaces; the corrective builder must then resolve RR-FND-001 through
RR-FND-003 before another re-review.
