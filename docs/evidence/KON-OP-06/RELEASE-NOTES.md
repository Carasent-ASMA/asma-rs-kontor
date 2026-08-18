# KON-OP-06 Release Notes

> **Date:** 2026-08-18
> **Status:** 🟢 Approved
> **Author:** Successor Architect seat
> **Category:** release
> **Scope:** ASMA-7875 / KON-OP-06 epic Completion Profiles
> **Summary:** Release evaluation for the contract-first Completion Profile compiler, durable completion state machine, remediation policy, closeout evidence model, and composed `/v1` completion operations.

---

## When to Load

**Load this document when:**

- releasing or integrating ASMA-7875 / KON-OP-06;
- assembling the real OP-04/OP-05 completion services in KON-OP-08;
- auditing the evidence behind the KON-OP-06 release verdict.

**Do NOT load for:** unrelated Kontor scheduling, Jira policy, or UI work.

---

## Release Verdict

**PASS — KON-OP-06 is approved for release.**

The ticket meets its contract-first acceptance criteria. The deterministic compiler and completion state machine are independently proven against typed task, integration, Committee, remediation, closeout, and wake contracts. The composed API fails closed where OP-08 has not yet connected the real OP-04/OP-05 services; it does not synthesize integration, Committee, or closeout success.

## Released Capability

- A strict, immutable Completion Profile contract with the built-in `operational_default@1` profile.
- Deterministic compilation to a bounded acyclic completion DAG: ticket evidence, Team C integration, up to two Committee rounds, one remediation round, closeout, and terminal `done` or `NEEDS_HUMAN`.
- Durable profile revisions, epic-pinned completion state, immutable round history, remediation proposals, command receipts, and exactly-once TPM wake intents.
- Composed `/v1` operations for profile list/preview/apply and epic completion read/advance/remediate.
- Role-separated remediation: the LSA proposes, the exact epic TPM routes, and neither receipt alone launches remediation.
- Evidence-gated closeout requiring merge, release, delivered-version inventory, final summary, notification, and archive receipts.
- Polyrepo integration records that preserve per-repository PR/module revisions and an optional root-pointer revision instead of assuming one branch or repository.
- Callback-first coordination with a finite declared polling fallback; no resident sleep loop or replacement TPM seat.

## Acceptance Evaluation

| Acceptance criterion | Verdict | Evidence |
| --- | --- | --- |
| Compiler and state machine pass independently before OP-04/OP-05 assembly, without copied feature implementation | PASS | `kontor-scheduler` completion tests exercise the typed ports directly; `kontor-policy` remains a pure `kontor-core` dependency; composed missing observations return typed `Unavailable` responses. |
| A failing Committee cannot close the epic and its evidence reaches the LSA | PASS | The fail-remediate-pass scenario emits `DeliverFailureToLsa`, remains outside closeout on failure, and requires a later typed passing verdict. |
| Only approved remediation launches | PASS | Remediation completion is refused before both the LSA proposal and exact TPM route exist; API tests cover authority, phase, replay, and stale revision ordering. |
| Prior rounds remain immutable; a second failure becomes `NEEDS_HUMAN` | PASS | Restart-at-every-stage coverage preserves round one, appends round two, and verifies a second failure carries two rounds of deliberation context. |
| Pass without every closeout prerequisite cannot become `done` | PASS | Policy tests require merge, release, version inventory, summary, notification, and archive; the state-machine scenario remains in closeout with all missing blockers until full evidence arrives. |
| Completion wakes the existing TPM once with no duplicate seat or sleep loop | PASS | Domain, repository, and API tests assert the exact TPM `SeatBindingId`, one wake per observation, replay without a second wake, and bounded polling exhaustion. |
| Completion works without invoking ASMA commands | PASS | The compiler/policy path has no external-process dependency; the completion application contains no ASMA command path, and uncomposed automated effects fail closed instead of falling back to `asma`. |
| Every `NEEDS_HUMAN` entry requires a resolution and tried deliberation path | PASS | Construction and deserialization reject an empty deliberation path; verdict exhaustion and polling exhaustion produce validated payloads. |

## Gate and Verification Evidence

- Code-review gate: **PASSED**, round 2, reviewed commit `3f7c373`; all original findings were remediated and the new regression tests were mutation-verified.
- QA gate: **ACCEPT**, `docs/evidence/KON-MVP-18/run-641600032c106a71/REPORT.md`, commit `7af730780fcb58ccc09c86e35c24ed7ace44b055`; **42 pass, 0 fail, 0 blocked, 0 missing**.
- Release-seat focused verification on 2026-08-18:
  - `cargo test -p kontor-scheduler --test completion` — 4 passed;
  - `cargo test -p kontor-policy --test completion_policy` — 3 passed;
  - completion-filtered and exact remediation tests in `kontor-daemon/tests/loopback_api.rs` — 5 passed.

## Release Boundary and Residual Notes

- KON-OP-08 remains responsible for connecting real OP-04 promotion/Core Team and OP-05 Committee services through the already checked ports. Until then, integration, Committee, and closeout observations without authoritative sources return `Unavailable`; this is the intended OP-06 boundary, not a hidden successful path.
- An authorized first advance may durably initialize a run before a later uncomposed observation refuses. Code review accepted this deterministic initialization as non-blocking; wrong-revision and unresolved-seat refusals create neither a run nor a receipt.
- The database-level append-only triggers for `command_receipts` were already absent before this ticket's migration. Application-level enforcement remains; the review recorded the pre-existing trigger gap as non-blocking and not introduced by KON-OP-06.

## Release Identity

- Jira: `ASMA-7875`
- Kontor task: `01a0074f-6729-7ad3-9194-0e38d5fffeb2`
- Branch: `feat/ASMA-7875-kontor-completion-profiles`
- Final release evaluation: **PASS**
