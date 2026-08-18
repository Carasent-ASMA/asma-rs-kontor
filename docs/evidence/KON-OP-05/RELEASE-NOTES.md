# KON-OP-05 / ASMA-7874 — release evaluation

> **Date:** 2026-08-18 09:10 CEST
> **Status:** 🔴 Release rejected
> **Author:** Successor Architect seat
> **Category:** report
> **Scope:** KON-OP-05 Advisors and configurable Committees
> **Summary:** Checkpoint 1 is internally coherent and its review and QA gates
> passed, but the admitted OP-05 task is not release-ready. Checkpoints 2–4 and
> every consultation run operation remain unimplemented.

---

## When to Load

**Load this document when:**

- evaluating or recording the KON-OP-05 release gate;
- resuming OP-05 implementation after the Checkpoint 1 gate sequence;
- checking whether Advisor or Committee consultations are operational.

**Do NOT load for:** evaluating the separately owned Completion, Jira cutover,
cross-feature CLI/MCP assembly, or diagnostic UI tickets.

---

## Verdict

**REJECT for the release gate.**

The code-review gate and QA gate are durably `passed`, but both evidence files
explicitly evaluate **Checkpoint 1 of 4**: typed/versioned profile publication,
not the admitted task's working Advisor and Committee consultations. The
release gate evaluates the task acceptance contract, so those narrower passes
cannot establish release readiness for OP-05 as a whole.

The current implementation still returns typed `Unavailable` from all five run
operations:

- Advisor invoke;
- Advisor settle;
- Committee invoke;
- Committee findings record;
- Committee settle.

`Services::resolve_scope` also still refuses `AdvisorConsultation` and
`CommitteeConsultation`. There is therefore no production path that creates an
ASW or CSW consultation, freezes context, records advice/findings, computes a
verdict, performs a bounded re-review, or enters `NEEDS_HUMAN`.

No release should be cut from this task state. The TPM should record this
architect verdict through the supported Kontor gate surface; this evaluator did
not mutate the gate.

## Evaluated state

| Evidence | Result |
| --- | --- |
| Kontor task | `01a0074f-6726-7823-be5a-719cc3d8ecc1`, revision 2, `in_progress` |
| Jira link | `ASMA-7874` |
| Code review gate | `passed` |
| QA gate | `passed` |
| Release gate before this evaluation | `not_ready`; requires `release-notes` |
| QA-frozen code revision | `867c6f97df48470bbe73a9e71f3a099fe2d8f9b1` |
| Implemented scope | Architecture Checkpoint 1 only |
| Unimplemented scope | Architecture Checkpoints 2, 3 and 4 |

The tracked tree was unchanged from the QA handoff before this release-note
artifact was added. Pre-existing untracked `docs/evidence/KON-MVP-18/run-*`
directories were left untouched and are not release evidence for OP-05.

## What Checkpoint 1 does deliver

- Closed `AdvisorProfileSpec` and `CommitteeTemplateSpec` definitions with
  canonical validation.
- Immutable, project-scoped profile/template publication through migration
  `0032`.
- Read, preview and apply operations for both definition families.
- The data-defined `independent_review@1` preset with two reviewers, one Judge,
  conjunctive aggregation, provider-family diversity and a two-round limit.
- Specification-level tests for two- and five-seat cardinality, provider-chain
  diversity and conjunctive outcome rules.
- Public API tests for pure preview, durable apply/read-back, replay,
  stale-revision refusal, cross-family refusal and no topology side effect from
  publication.

This is useful enabling work, but it is not a releasable implementation of the
OP-05 task goal.

## Acceptance assessment

| Admitted acceptance criterion | Result | Release evidence |
| --- | --- | --- |
| ASW placement is epic-local, idempotent and never inside TSW/CSW | **Not met** | Advisor invocation is `Unavailable`; no ASW is placed. |
| CSW supports ticket or epic scope without changing node kind | **Not met** | Committee invocation is `Unavailable`; no CSW run is placed. |
| Variable 2/3/5-member Committee fixtures work | **Not met end to end** | Two-/five-seat specifications validate, but no Committee run path exists. |
| Diversity/quorum failures block before launch | **Not met end to end** | Definition validation exists; there is no launch to guard and no durable quorum execution. |
| Judge cannot finalize early | **Not met end to end** | A pure conjunction helper has missing-finding coverage, but findings and settle operations are unavailable. |
| Missing evidence counts against the gate | **Partially met** | The pure outcome rule treats incomplete evidence as non-compliant; no persisted Committee settlement consumes it. |
| Advisor/Committee cannot invoke mutation or raw topology tools | **Partially met** | The definitions cannot name mutation, but no consultative seat/capability boundary is composed or exercised. |
| Results are template-driven rather than hard-coded to three seats | **Partially met** | Specification fixtures cover two and five seats; the generic invocation/settlement path is absent. |
| New state emits CSW and historical TSC resolves without duplication | **Partially met** | Identifier normalization exists, but no new Committee state is emitted or reconciled. |
| Inconclusive paths enter `NEEDS_HUMAN` with recommendation and tried path | **Not met** | Checkpoint 4 is explicitly unimplemented. |

The explicit protocol deferrals (Jury, Conjunctive Compliance, Deliberative
Panel and interactive debate) remain legitimate out-of-scope items. The
Advisor/Committee run behavior in Checkpoints 2–4 is not a deferral: it is the
core accepted scope of OP-05.

## Additional release blockers to close during completion

The passed Checkpoint 1 review preserves findings that become material before
the run service is composed:

1. Reconcile a published canonical profile whose receipt write was interrupted,
   so retry does not misreport the caller's own completed publication as a
   revision conflict.
2. Reject definitions whose caller or slot role cannot resolve to a usable
   catalog role before publishing an immutable, unconvenable revision.
3. Restore and schema-test the inherited `command_receipts` immutability and
   no-delete triggers at the next receipt-table rebuild.
4. Re-evaluate whether provider independence may be disabled for a Committee
   template, and make the accepted rule explicit before launch behavior ships.

## Required remediation before release re-evaluation

1. Complete architecture Checkpoints 2–4 through the existing OP-03
   application operations and OP-02 semantic topology path.
2. Replace the five run-operation stubs with repository-backed,
   restart-reconcilable behavior and enable consultation scope resolution only
   after durable run-to-epic identity exists.
3. Prove the admitted placement, authority, context-freezing, variable
   cardinality, independent-finding, Judge-ordering, evidence, round-lineage,
   seat-reuse, TSC-alias and `NEEDS_HUMAN` requirements through public
   operation tests.
4. Close or explicitly disposition the preserved review findings, update the
   implementation handoff to Checkpoint 4 of 4, and rerun code review and QA
   against the completed scope.
5. Request a fresh architect release evaluation. The TPM then records that
   verdict through Kontor.

