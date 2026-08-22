# KON-OP-10 / ASMA-7879 — Release Notes

> **Date:** 2026-08-22
> **Status:** verification passed for the deterministic admission core
> **Scope:** `asma-rs-kontor` PR #87, head `4a0ef58ac6e4055106e263b3f6de5e4d736c931f`
> **Summary:** Isolated scheduler proof of the operational 4→5→6→7 climb and
> eighth-run mission-ceiling refusal. No production code change. Live
> isolated-home / disposable Jira / QNR v2 pilot remains an explicit remainder.

## When to Load

**Load this document when:** recording the KON-OP-10 `release-gate`, or
asking whether the seven-run ceiling is proven in-process.

**Do NOT load for:** a live Paseo/Jira fourteen-step receipt, or QNR v2
cutover. Those are not in this release.

## What ships

- One scheduler integration test that admits four on a fresh window
  (`initial=4`, `ceiling=7`, `growth_step=1`, `mission=7`), then one newly
  eligible candidate at 5, 6 and 7, then refuses the eighth with
  `capacity_exhausted` and `RejectionEvidence::Capacity { limit: Mission,
  remaining: 0 }`.
- Evidence matrix pointing at existing accounts-fold, loopback idle-seat,
  and pressure-contraction tests for the rest of EVD-OP-012.

## Verification

| Check | Result |
| --- | --- |
| `cargo test -p kontor-scheduler --test ready_batch` | 33 passed |
| `cargo test -p kontor-scheduler` | passed |
| `cargo test -p kontor-accounts admission` | 7 passed |
| Inspector [REVIEW-NOTES.md](REVIEW-NOTES.md) | PASS (pinned scope) |
| Tester [QA-REPORT.md](QA-REPORT.md) | PASS |

## Deliberate remainder

The plan's live isolated-user-home journey, disposable Jira
create/link/cleanup, and QNR v2 opt-in ticket are not claimed. OP-11
independently reproduces the seven-run proof and dispositions remaining
Operational Gaps. Superproject gitlink `_tools/asma-rs-kontor` is not
advanced by this ticket.

## Verdict

**PASS for release-gate** on the deterministic admission core. The architect
records the gate; this document is the `release-notes` artifact.
