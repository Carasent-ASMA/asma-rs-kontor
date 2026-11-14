# Admit a milestone on a launch-ready check scoped to its rows

> **Status:** Accepted
> **Date:** 2026-11-14 07:20 UTC
> **Authority:** Configured platform owner
> **Scope:** Execution admission of a milestone in a plan that selects milestone delivery. Plans without milestone delivery, structural validation, final epic closure and remote binding checks are unchanged.
> **Supersedes:** none
> **Publication class:** P

## Context

The admission gate requires the launch-ready plan check to pass on the whole
saved plan. That check requires a tracker binding on the epic and on every task
row ([plan-graph contract](https://github.com/Carasent-ASMA/asma-rs-kontor/blob/master/workflow/skills/create-implementation-plan/references/plan-graph-contract.md#author-one-source)).

Milestone delivery
([delivery milestones](https://github.com/Carasent-ASMA/asma-rs-kontor/blob/master/workflow/skills/create-implementation-plan/references/delivery-milestones-and-adoption.md))
defines later milestones only shortly before they are scheduled. Until its last
milestone is defined, a milestone plan cannot pass the whole-plan check. That
leaves two bad choices: create tracker issues for scope that is not defined
yet, which breaks the rule that bindings are never invented, or hold every
milestone until the end, which defeats milestone delivery.

<a id="DEC-01"></a>

## Decision

- A plan whose frontmatter selects exactly one `delivery_strategy: milestone`
  may admit one milestone on a launch-ready check scoped to that milestone's
  rows, when the project validator offers a scoped mode.
- The scoped check keeps whole-graph structural validation. It requires bindings
  on the epic, on the named tasks and on the transitive closure of their
  `depends_on`, plus Kontor IDs for the same set on governed work. It refuses an
  empty or unknown scope, a plan without milestone delivery, and a scoped run
  over more than one plan. Admission also refuses a scope that does not equal
  exactly one milestone's mapped rows; without a milestone field the validator
  cannot detect that, so the coordinator checks the printed scope.
- The scope equals the milestone's mapped rows on the authoritative saved
  revision. The PASS line and the receipt name the scope and its dependency
  closure. Rows outside that closure need not be bound: existing confirmed
  bindings are preserved, and unbound future rows may remain `null`. Each
  milestone needs its own scoped receipt.
- Final epic closure requires an unscoped launch-ready PASS.
- The scope is an explicit task list. The plan graph gets no milestone field.

The plan-graph contract's
[milestone-scoped launch check](https://github.com/Carasent-ASMA/asma-rs-kontor/blob/master/workflow/skills/create-implementation-plan/references/plan-graph-contract.md#milestone-scoped-launch-check)
holds the full rule. Epic orchestration and the milestone procedure point to it.

## Consequences

- A milestone plan can admit its first milestone without inventing tracker
  issues for later scope.
- A scoped receipt is visibly different from whole-plan readiness, because its
  PASS line names the scope.
- The receipt is only as good as the scope the coordinator passes. The rule that
  the scope equals the milestone's rows, and the scope printed in the PASS line,
  make that reviewable.
- A project whose validator has no scoped mode keeps the whole-plan check.
- Superseding this decision requires changing the plan-graph contract, the
  epic-orchestration entry gate and closeout, the milestone procedure and every
  project validator that implements the scoped mode.
- Upgrade path: a milestone field in a later graph schema version, once a second
  plan uses milestones.

## Alternatives considered

- **Bind every row now:** creates issues for scope that is not defined, then
  needs re-sync work, and contradicts the rule against invented bindings.
- **One plan per milestone:** breaks the single dependency graph and its
  cross-milestone edges, and duplicates the epic plan.
- **A hand-written scoped receipt or a waiver:** the contract forbids a waived
  validator and invented PASS evidence.

## Evidence

- Baseline reviewed: `7cc097c9`.
- Acceptance: the configured platform owner's explicit 2026-11-14 instruction
  approves this rule, including two refinements that narrow it: the milestone
  frontmatter check and the dependency closure in the PASS line.
- Implementation qualification belongs to the delivering changes' exact-revision
  review and test evidence. This record claims no live qualification.
