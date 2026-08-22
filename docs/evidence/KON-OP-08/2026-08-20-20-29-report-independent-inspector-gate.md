# KON-OP-08 / ASMA-7877 — independent inspector gate

> **Date:** 2026-08-20 20:29 CEST
> **Status:** 🔴 Rejected
> **Author:** Inspector · KON-OP-08
> **Category:** report
> **Scope:** task revision 2, TeamRun `01a0195b-7280-7500-81cf-c28023f8cbf8`

## Summary

PR 64 is **rejected** at the independent code-review gate. The implemented
dynamic-scope and seat-lifecycle slice is green, but the candidate does not
satisfy the approved OP-08 contract. The exact false-success Jira receipt class
that the architecture calls out remains possible, project-owned field/workflow
pair pinning and the wider approved control-surface scope are absent, and the
materialization repair path can leave a pre-existing ticket node without its
required ECP node.

This verdict authorizes neither merge nor scope reduction. Re-review requires a
new immutable candidate that corrects all blocking findings and preserves the
accepted behavior already present.

## When to load this report

Load this report when correcting or re-reviewing OP-08 revision 2, or when the
TPM reconciles its code-review gate in Kontor.

Do **not** use it as approval for PR 64, as a replacement for the approved
architecture, or as evidence that the Jira issue was reconciled.

## Inspected identity

| Item | Immutable value |
| --- | --- |
| Kontor realm | `01a00649-9ee6-73e0-ba1b-6a6c35cfd065` |
| Kontor project | `01a0064a-e056-7603-9968-ef64fdaacb75` |
| Kontor epic | `01a0074f-6719-7570-adf7-95ee3ec69875` |
| Kontor task | `01a0074f-672e-79a3-9876-d0e1bf585d4e` revision 2 |
| TeamRun | `01a0195b-7280-7500-81cf-c28023f8cbf8` |
| Inspector AgentRun | `01a01ead-7837-7bf1-b63b-cd596c9b0d97` |
| Inspector SeatBinding | `01a01ead-7837-7bf1-b63b-cd596c9b0d96` |
| Native seat | `14fdc0c8-056b-451b-8d50-d04906216b94` |
| Pull request | [PR 64](https://github.com/Carasent-ASMA/asma-rs-kontor/pull/64) |
| Candidate head | `4fb0386e2809b0d7af6f90e9e58284cc6289dac8` |
| Inspected base | `6724ad60ac991a4de9087c53eb6de4d8e44e0ade` (`origin/master`) |

The candidate contains 10 changed files, 2,337 insertions and 28 deletions. Six
changed files are Rust source/tests and four are OP-08 evidence documents. The
three untracked `docs/evidence/KON-MVP-18/run-*` directories were not read as
candidate evidence, changed, staged or committed.

## Verdict: rejected

### FND-001 — Jira apply still manufactures convergence after one staged hop

Severity: **blocking**.

The approved architecture requires a fresh post-effect readback and a closed
`converged`, `progressed` or `unconfirmed` disposition. It explicitly forbids
overloading a non-empty `converged` collection with "an effect was attempted"
(`2026-08-19-11-39-architecture-kontor-operational-control-surfaces.md`, lines
311-327) and requires the known false-success receipt shape to be rejected
(lines 336-354).

The candidate still does the opposite:

- `Services::ticket_reconcile_apply` invokes the planned effect and stores the
  returned confirmation, then unconditionally returns every planned link in
  `converged` (`crates/kontor-daemon/src/applications.rs`, lines 14243-14382).
- `TicketDelegation::apply` requires a confirmation object but never verifies
  that its observed status is the declared intermediate or final milestone
  (`crates/kontor-integrations-asma/src/jira.rs`, lines 918-944).
- `StatusTransitionReceipt::validate` checks only that a claimed confirmation
  names an observation id; it does not validate the confirmed destination
  (`crates/kontor-core/src/ticket.rs`, lines 1366-1380).
- The new staged-hop assertion covers dry-run plan shape. It does not exercise
  the public apply response or reject the false-success receipt.

Consequently, a live `DRAFT -> READY FOR DEVELOPMENT` first hop can still be
reported as final convergence before a fresh observation proves `In
Development`. Green tests do not establish the required behavior because no
test enters this path and asserts the closed readback disposition.

Required correction: expose and persist the post-effect disposition and exact
observation, validate the readback against both attempted destination and final
milestone, reject contradictory or absent readback, and add the full two-hop,
lost-acknowledgement and false-success receipt regressions required by the
approved architecture.

### FND-002 — the approved OP-08 control-surface scope is not implemented

Severity: **blocking**.

No durable approved supersession was found for the OP-08 architecture committed
in this candidate. Dropping functionality already delivered by OP-18 avoids
duplicate work; it does not remove the remaining OP-08 requirements. Concrete
missing acceptance items include:

- `Services::jira_specs` still selects
  `catalog.field_specs().first()` and
  `catalog.workflow_specs().first()`
  (`crates/kontor-daemon/src/applications.rs`, lines 1490-1515), contrary to the
  explicit exact project pair-pin contract at architecture lines 287-309.
- The CLI exposes workflow-spec installation from OP-18, but no separate Jira
  field-spec install and no atomic compatible pair-pin operation/readback.
- `AsmaExecutable` and the `kontor-integrations-asma` subprocess boundary remain
  composed in production. The required zero-reference scan is non-zero in
  `crates/kontor-integrations-asma/src/process.rs`, its Jira boundary,
  `crates/kontor-daemon/src/lib.rs` and
  `crates/kontor-daemon/src/applications.rs`.
- The candidate changes no CLI/MCP/bootstrap/ASMA-forwarder implementation
  files. The approved checkpoints still require full registry `/v1`/CLI/MCP
  parity, completion assembly, bootstrap/update/client registration, fixed ASMA
  forwarders followed by subprocess deletion, and the installed primary journey
  (architecture lines 747-809 and 811-839).

The builder report accurately describes the smaller slice it proved, but that
slice is not the accepted task. Passing CI and behavioral mutants for only that
slice cannot satisfy unimplemented acceptance criteria.

Required correction: either implement the entire still-approved OP-08 scope, or
obtain and commit an explicit architecture/task supersession that accounts for
every removed requirement before presenting a new candidate. A handoff summary
alone is not a scope authority record.

### FND-003 — materialization cannot repair a legacy ticket node missing its ECP

Severity: **blocking**.

Before this candidate, ticket materialization used `ensure_scope_chain`, which
could persist a root/epic/task chain without the ECP node. The candidate routes a
new materialization through `ensure_task_node`, but that function immediately
returns an existing task node before ensuring the ECP
(`crates/kontor-daemon/src/applications.rs`, lines 17456-17466). In addition, an
exact idempotency replay skips the reconciliation body entirely (lines
8952-8988).

Therefore a task node produced before this change can remain permanently
unrepaired: exact replay does no work, and a new-key materialization returns the
task node before creating or re-attesting its ECP. Later seat admission then has
no durable control-plane parent. The new public integration test starts from an
empty daemon and does not cover this upgrade/re-attestation state.

Required correction: make the ensure path validate and repair the whole durable
chain even when the task leaf already exists, define replay readback behavior
for that repair, and add an upgrade regression seeded with an existing task node
and missing ECP.

## Independent verification

The following evidence is accepted as green for the implemented slice:

- all four GitHub checks on the exact head passed:
  Console gates jobs `96528205361` and `96528181017`; Rust workspace gates jobs
  `96528205621` and `96528181090`;
- `cargo fmt --all -- --check` passed independently;
- `cargo test -p kontor-core -p kontor-integrations-asma --all-targets` passed;
- focused daemon regressions passed independently:
  `an_applied_task_materializes_and_replays_without_a_startup_task_scope`,
  `a_terminal_agent_run_is_not_the_live_seat_of_its_still_open_team`, and
  `a_team_closes_on_settled_turns_while_every_seat_stays_live`;
- the builder's pinned-gate and eight restored-mutation evidence is recorded in
  `2026-08-20-13-18-report-dynamic-scope-and-seat-lifecycle-proof.md`.

One evidence discrepancy is non-blocking by itself but must be corrected:
`git diff --check 6724ad60..4fb0386e` exits 2 because the two earlier OP-08
metadata blocks contain trailing whitespace. The handoff's unqualified
"diff-check passed" does not describe the committed candidate range.

Jira reconciliation remains unverified. The builder recorded that
`asma jira sync --ticket ASMA-7877 --dry-run` refused because `JIRA_BASE_URL` is
unset; this review inferred no Jira state.

## Open-question ledger

### OQ-001 — evaluator account for the current inspector gate

- **Subject:** which account profile is authorized to record this inspector
  verdict.
- **Attached record:** this report; Kontor task
  `01a0074f-672e-79a3-9876-d0e1bf585d4e`; TeamRun
  `01a0195b-7280-7500-81cf-c28023f8cbf8`; Inspector AgentRun
  `01a01ead-7837-7bf1-b63b-cd596c9b0d97`.
- **Why ambiguous:** live readback at snapshot cursor 356 reports
  `account_profile_id: null` for both this inspector run and its retired
  predecessor. `kontor gate-record` requires `--evaluator-account`; inventing or
  borrowing an account would misattribute the verdict. The task is also still
  in `implementation`, its declared `code-review-gate` is `not_ready`, the
  TeamRun is `queued`, and its successor builder remains attached.
- **Options observed:** (1) the owning TPM settles the builder handoff, advances
  the task and explicitly attaches the authenticated inspector account to this
  same run; (2) an already-authorized account is explicitly assigned and read
  back through a supported Kontor operation; or (3) the Kontor owner corrects
  the accountless-successor/gate-record contract and redeploys before retry.
- **Disposition:** unresolved and blocking the typed control-plane receipt. No
  option was assumed and no Paseo or state-file fallback was used.

## Kontor recording checkpoint

Intended operation: record `code-review-gate = rejected` for task revision 2,
with evaluator role `inspector`, this report plus the candidate head as evidence,
and a stable idempotency key. The operation cannot be safely formed while
OQ-001 is unresolved, and the gate is not ready to accept a verdict.

Failure class: `identity_missing` plus `gate_not_ready`. Scope identities are the
realm/project/epic/task/TeamRun/AgentRun values above. GitHub refused a formal
request-changes review because the authenticated account owns PR 64. The
bounded fallback was the complete rejected report as durable
[PR comment 5360128453](https://github.com/Carasent-ASMA/asma-rs-kontor/pull/64#issuecomment-5360128453);
it changed no Kontor, Paseo, Jira, candidate code/head or merge state. Current
checkpoint: the TPM or Kontor subsystem owner must resolve OQ-001 and settle the
implementation handoff, then preserve this rejected verdict as the review
evidence for the corrective run. Status: **open**.

## Re-review entry criteria

A new inspection may begin only after all three findings have corrective
commits and behavior-level regressions, the approved scope or its durable
supersession is explicit, the candidate base/head pair is frozen, and Kontor
reads back a ready code-review gate with an attributable evaluator identity.
