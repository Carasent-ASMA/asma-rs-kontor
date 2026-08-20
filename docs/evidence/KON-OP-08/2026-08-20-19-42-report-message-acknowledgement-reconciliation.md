# OP-08 Message Acknowledgement Reconciliation

> **Date:** 2026-08-20 19:42
> **Status:** 🟡 In Review
> **Category:** report
> **Scope:** ASMA-7869 / KON-OP-08 / Paseo session-message delivery
> **Summary:** Records the live false-negative acknowledgement that occurred while resuming the exact OP-08 builder, its backward-timeline reconciliation repair, and the evidence that no replacement or duplicate message was authorized.

---

## When to Load

**Load this document when:**

- a Kontor `session-message-send` returns unavailable although Paseo started the exact turn;
- changing Paseo canonical-timeline paging or message idempotency reconciliation;
- verifying that OP-08 resumed on its preserved builder identity.

**Do NOT load for:** ordinary delivery after the message acknowledgement has reconciled, or QNR product implementation.

---

## Live incident

Kontor addressed AgentRun `01a01eb0-453d-7c21-ac60-bee1d8cf8d73`, runtime
binding `01a01eb0-453d-7c21-ac60-bee1d8cf8d72`, native Paseo agent
`1743804b-f9b9-4167-98ad-e39b3f402a01`, generation 1. The supported
`session-message-send` returned HTTP 503, and a same-key reconciliation also
failed. Admin replacement was correctly refused because the fresh runtime
directory still called the predecessor reachable.

A bounded read-only Paseo diagnostic, taken only because the Kontor readback
contradicted its own send result, proved that the same native seat was running
the delivered turn in workspace `wks_162d4d0509f255c1`, on the preserved
`feat/ASMA-7877-kontor-operational-control-surfaces` branch and OP-08 worktree.
The provider thread and native agent identity were unchanged. No direct Paseo
mutation, new seat, replacement, or duplicate topology was created.

## Root cause

Paseo's cursor-free `tail` timeline read returns the newest bounded page. Kontor
correctly used that read after a lost or delayed send acknowledgement, but its
reconciliation loop then followed `hasNewer` with `after(endCursor)`. There can
be no newer page after the newest tail window. If an active turn emitted more
than 100 entries before acknowledgement settlement, the accepted user message
fell onto an older page and Kontor could not find it.

The false negative left the delivery as `ConfirmationUnknown` and mapped the
safe uncertainty to HTTP 503 even while the exact native turn was executing.
Replacing that seat would have duplicated active work, so replacement was not
performed.

## Correction

Message and permission reconciliation still begin at the newest canonical tail
window, but now walk backward through `hasOlder` and the runtime-issued
`startCursor` using direction `before`. The adapter verifies that each response
echoes the requested direction, preserves the existing epoch-continuity and
sequence-gap refusals, and records the acknowledgement only when the exact
`clientMessageId` is found exactly once.

Recorded timeline fixtures now describe the direction in which each fixture is
actually consumed. The public history and live-subscription paths retain their
existing `tail`/`after` semantics.

## Regression and mutation evidence

`message_a_lost_ack_older_than_the_tail_page_is_reconciled_backwards` reproduces
the live shape: a newest 100-entry page does not contain the message, while the
older page contains its exact id. Before the repair the focused test failed
with the lost-acknowledgement transport error. After the repair it returns the
message's native position, performs two timeline reads, and proves the send RPC
ran exactly once.

Mutation M31 changed the second-page direction from `before` back to `after`.
The same focused regression failed with `CorrelationFailed`, killing the
mutant. The original source was restored immediately, the focused regression
returned green, and all 137 Paseo contract tests passed. No mutant remains.

## Release checkpoint

The isolated branch is `fix/ASMA-7877-recover-unreachable-delivery-seat` on
master merge `6724ad60ac991a4de9087c53eb6de4d8e44e0ade`. The release candidate passed:

```text
cargo fmt --all -- --check                            passed
git diff --check                                       passed
cargo clippy --workspace --all-targets -- -D warnings passed
cargo test --workspace                                passed
pnpm --dir apps/console verify:api                     passed
pnpm --dir apps/console typecheck                      passed
pnpm --dir apps/console test                           passed (295 tests)
cargo audit                                             passed (allowed advisories only)
cargo deny check                                        passed
pnpm audit --prod                                       passed (no known vulnerabilities)
cargo build --release --workspace                     passed
```

PR/CI, deployment, same-key live reconciliation, and the final OP-08 inspector
handoff receipt will be appended before this report is approved.
