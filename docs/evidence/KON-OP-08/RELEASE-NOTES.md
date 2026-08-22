# KON-OP-08 / ASMA-7877 — release notes

> **Date:** 2026-08-22
> **Status:** 🟢 Approved
> **Author:** Architect · OP-08 (LSA recording)
> **Category:** report
> **Scope:** `KON-OP-08` Operational control surfaces and ASMA compatibility
> **Summary:** Release evidence after independent code-review and QA gates.
> The accepted production unit is PR 64 on `master` plus the
> `DeliveryConfirmationUnknown` follow-up on this branch.

---

## When to Load

**Load this document when:**

- evaluating or replaying the `KON-OP-08` `release-gate`;
- reconciling `ASMA-7877` with the Kontor task projection; or
- consuming OP-08 as an OP-10 / OP-13 integration dependency.

**Do NOT load for:** redesigning `/v1` or reopening inspector findings already
classified non-blocking.

---

## Release verdict

**PASS for `release-gate` evaluation.** The frozen `code@1` profile reserves
this verdict for `architect` and requires the `release-notes` artifact.

| Unit | Revision | Disposition |
| --- | --- | --- |
| PR 64 merge | `527cc164e822d0cd9cc03aa8c53d2f532a80cea9` | Operational control surfaces on `master` |
| Replay-convergence fix | `d310cdc2be77110b7b88b9fec38d8804aceba05d` | already in that merge |
| Delivery confirmation unknown | this branch (`fix/ASMA-7877-delivery-confirmation-unknown`) | incomplete history scan must not authorize a resend; QA-passed |

## Durable gate evidence

Kontor project `01a0064a-e056-7603-9968-ef64fdaacb75`, task
`01a0074f-672e-79a3-9876-d0e1bf585d4e`, TeamRun
`01a0195b-7280-7500-81cf-c28023f8cbf8`:

| Gate | Evidence | Result |
| --- | --- | --- |
| `code-review-gate` | inspector PASS on the PR 64 unit; findings F1–F7 recorded as non-blocking | passed |
| `qa-gate` | `docs/evidence/KON-OP-08/QA-REPORT-2026-08-22.md`; receipt `01a02677-428f-7690-aea0-e73957ec4b1c` | passed (1640 tests, 0 failed, clippy/fmt clean, including the three DeliveryConfirmationUnknown tests) |
| `release-gate` | this `release-notes` artifact | architect pass |

## Released behavior

- Agents operate through Kontor MCP/`/v1` without a UI, ASMA workflow command or
  direct runtime/Jira access.
- CLI and MCP share one registry; CLI emits one JSON document.
- Staged Jira hops send a destination that matches the hop, not the milestone.
- An incomplete Paseo history confirmation is `Unavailable` and does not claim
  that nothing changed, so the caller must not resend until canonical history
  proves the outcome.

## Deliberate limitations

- Inspector findings F1–F7 remain uncovered by the suite and are non-blocking
  follow-ups. F1 (`409 placement_blocked` after a per-epic topology upgrade)
  is the one to ticket first.
- Eight ignored live-harness tests were not run (live Paseo/AO/Codex).
- Uncommitted confirmation work is released only with this follow-up branch.

## Data, compatibility and rollout

No schema migration. Existing TeamRuns, seats and bindings are preserved.
Jira status follows the Kontor task through typed reconciliation after this
gate and the follow-up merge are durable.
