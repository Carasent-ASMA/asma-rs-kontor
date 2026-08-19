# KON-OP-12 / ASMA-7881 — Release Notes

> **Date:** 2026-08-19 19:10 CEST
> **Status:** 🟢 Verification passed
> **Author:** Verifier · KON-OP-12
> **Category:** report
> **Scope:** `asma-rs-kontor` PR #45, merged as `2c4e5e495ae7ad826e389cadb30adba3f615f3ac`
> **Summary:** Release evidence for the durable open-question ledger, its
> append-only disposition history, report-only detector and completion blocker.
> The verifier recommends passing the release gate within the ticket's pinned
> scope; the architect remains the gate authority.

---

## When to Load

**Load this document when:**

- evaluating or recording the KON-OP-12 `release-gate`;
- integrating schema generation 41 or the additive completion-blocker contract;
- consuming the open-question repository, detector or completion-state seam.

**Do NOT load for:** designing the later public raise/dispose surface or the
missing daemon closeout driver; those are explicitly outside this ticket.

---

## Release identity

| Item | Revision |
| --- | --- |
| Implementation | `a883f00` |
| Builder evidence | `9ca79f6d9b604ef639aae3d68d858cf6c9203268` |
| Merged PR | [#45](https://github.com/Carasent-ASMA/asma-rs-kontor/pull/45), merge `2c4e5e495ae7ad826e389cadb30adba3f615f3ac` |
| Inspector/tester evidence commit | `4b0400cca582047b5765de7d476c64eb7dd6897d` |
| Canonical scope | OP-REQ-038 / KON-OP-12 in the Operational MVP plan |

No new platform-wide ADR is introduced. The release implements the already
pinned, Kontor-local OP-REQ-038 contract and does not change cross-service
topology governed by ADR-0008.

## What ships

- A project-scoped, epic-attached `OpenQuestion` aggregate with an immutable
  header and append-only ambiguity rounds, dispositions and trigger firings.
- Exactly three dispositions: `resolved` with an exact decision citation,
  `deferred` with a concrete reopening trigger, and `not_relevant` with a
  reason. Corrections supersede history instead of rewriting it.
- Explicit reopening when the current deferral's exact trigger fires, including
  protection against stale firings resurrecting a later disposition.
- Data-defined closer authority: architecture/product questions use the
  configured architecture closer; process/routing questions use the configured
  process closer. Raising remains available to any ordinary seat.
- A pure, report-only detector for contradictory accepted decisions,
  superseded citations and fired deferral triggers. It receives no mutation or
  repository port.
- Schema generation 41 with one head table and three immutable child tables,
  compare-and-swap repository operations, composite project keys and
  deterministic export/import/snapshot preservation.
- A typed completion blocker carrying the question id and subject. Open and
  reopened questions withhold `MarkDone`; resolved, deferred-with-unfired-
  trigger and not-relevant questions release the gate.
- The open-question duty in every ordinary seat's resolved prompt, without a
  scanner, standing run, notification path or auditor role.

## Compatibility and migration

Schema generation advances from 40 to **41** through
`0041_open_questions.sql`. Existing databases migrate in place; downgrade is
not supported, consistent with the existing migration contract.

The release also repairs the explicit historical-v35 convergence list so it
applies `MIGRATIONS[40]`. Without that entry, a database on that historical
lineage would stop at v40 and then refuse to open. The dedicated lineage test
proves the database reaches v41 without losing its receipt.

The OpenAPI change is additive: two new `CompletionBlockerDto` `oneOf` members,
`OpenQuestionUndispositioned` and `OpenQuestionReopened`. Generated console
types are synchronized. No route, CLI operation or MCP tool is added.

## Verification

The inspector recorded **PASS** in [REVIEW-NOTES.md](REVIEW-NOTES.md), and the
tester recorded **PASS** in [QA-REPORT.md](QA-REPORT.md). The builder's broader
[QA.md](QA.md) reports formatting and Clippy clean, 1,547 workspace tests
passing, generated contract parity, and every mutation target caught.

The verifier independently reran the acceptance-critical release slice at
evidence head `4b0400c`:

| Check | Result |
| --- | --- |
| `cargo test -p kontor-core --test open_question` | 27 passed, 0 failed |
| `cargo test -p kontor-scheduler --test completion` | 10 passed, 0 failed |
| `cargo test -p kontor-store --test open_questions` | 12 passed, 0 failed |
| historical v35→v41 convergence test | 1 passed, 0 failed |
| `pnpm --filter kontor-console verify:api` | generated types match the committed OpenAPI contract |

## Deliberate boundaries and integration facts

1. There is no public API/CLI/MCP raise or dispose surface. This is required by
   the pinned prohibition, not an omitted ticket deliverable. OP-08 owns the
   later agent-reachable write surface and `CloserPolicy` construction.
2. `CloseoutRecorded` still has no production daemon construction path. That
   gap exists on the implementation's master baseline and was neither
   introduced nor widened by PR #45. This release strengthens the seam by
   making `open_questions` mandatory wherever that signal is eventually
   composed; OP-06/OP-11 integration owns the driver.
3. OP-08 had independently claimed migration number 0041. PR #45 merged first,
   so OP-12 keeps 0041 and OP-08 must integrate this merge and renumber to the
   next free generation before publishing its migration.
4. This worktree's superproject gitlink still names its old bootstrap revision.
   The ticket release is the merged submodule PR above; advancing the
   superproject pointer belongs to the serialized epic integration step.

## Verdict

**PASS for release-gate evaluation.** The shipped change satisfies the pinned
KON-OP-12 scope, both prerequisite gates passed, and the independent release
slice is green. The verifier supplies `release-notes`; the `architect` role
alone records the final gate verdict.
