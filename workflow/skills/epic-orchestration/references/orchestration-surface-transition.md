# Orchestration surface transition policy

This is the detailed owner of the transition rule. The project entry file keeps
a short pointer to the stable `#orchestration-surface-transition-rule` anchor.

<a id="orchestration-surface-transition-rule"></a>
### Orchestration surface (transition rule)

`kontord` is not yet trusted as the everyday control plane in every project.
Until `{{ project.decision_authorities.platform }}` retires this section by a
dated commit, the default orchestration surface is **Paseo-direct**, and
`kontord` governs an epic only when it is explicitly marked.

- **One workflow, two lasting surfaces.** Both surfaces run the unified Kontor
  workflow. What each surface adds, and why the workflow leads and `kontord`
  follows, is in the [workflow guide](../../../../docs/WORKFLOW.md#the-kontor-workflow).
  What a bare "Kontor" means is in the
  [glossary](../../../../docs/GLOSSARY.md#the-kontor-value).
- **Plan spelling and compatibility.** New or explicitly amended plans state
  `orchestration: paseo` or `orchestration: kontor`, with the selected value and
  an italic explanation near the start. `paseo` maps to Paseo-direct; older
  `paseo-direct` and `kontor-governed` values retain their meanings. A missing
  field remains Paseo-direct. A `kontor` plan also retains
  `Orchestration: kontor-governed` and the paired tracker label below; the short
  field alone does not activate governance or transfer a live run. This
  documentation does not change existing runtime parsers or pins.
- **Workflow field.** `workflow: kontor | free` (default `kontor`) is
  independent of the surface; `workflow: free` with `orchestration: kontor` is
  forbidden. Follow the [workflow field](../../../../docs/WORKFLOW.md#1-words-before-rules).
- **Marker.** An epic is Kontor-governed when its tracker epic carries the
  label `kontor-governed` **and** its plan states `Orchestration:
  kontor-governed` in the frontmatter or first section. Either alone is a
  request to add the other, not a selection; the word "Kontor" or a feature
  hosted in the Kontor app selects nothing. Everything else, including every
  epic that predates this rule and has no live `kontord` run, is Paseo-direct.
- **Paseo-direct (default).** Plan, tracker epic and child issues, Paseo
  placement per the [native placement reference](paseo-direct-native-placement.md),
  the project CLI's branch/commit/PR grammar, worktree discipline and
  committees stay exactly as they are; only the control plane above them is
  gone. Create tracker issues through the project CLI's tracker operation, move
  status through the supported tracker writer, and record decisions in the plan
  and the PR. `kontord` unavailability never blocks: no containment action, no
  repair before resume, no checkpoint receipt, no attestation gate. The project
  CLI treats publication attestation as advisory and writes the tracker itself
  on this surface; a `kontord`-launched seat carries its own authentication and
  runs as `kontor`.
- **Kontor-governed (opt-in).** The pre-transition rules apply unchanged for
  that epic: Kontor MCP or the supported Kontor CLI over `/v1` for orchestration
  reads and mutations; Paseo direct only as one bounded containment action
  limited to the blocked operation; every fallback recorded as typed
  `operational_gap` evidence on the owning Kontor task or epic, or as a dated
  repository gap report when `kontord` cannot accept it; the gap fixed end to
  end by its owner in an existing seat (root cause, smallest regression test,
  correction, contract suites, redeploy, readback); and the workflow resumed
  from the recorded checkpoint. Placement and naming are rendered by `kontord`
  from the pinned Team Definition; do not place by hand. The orchestration
  steps live in [Kontor-governed epic orchestration](kontor-governed.md).
- **Correction preference.** When a repair includes a correction, prefer this
  order: remove the faulty path, then a type or schema, then a guard, then a
  regression test built from the real failing case; prose last. It supplements,
  never replaces, the repair steps above: the regression test, redeploy and
  readback remain mandatory.
- **Recorded gaps stand, new defects are logged.** Every existing gap report in
  the project's gap ledger (`{{ project.gap_ledger }}`) and every typed
  `operational_gap` on a Kontor task remains the `kontord` repair backlog,
  closed with a root cause, the smallest regression test, a correction and a
  redeploy. An agent on a Paseo-direct epic that observes a `kontord` defect
  adds one row to the ledger, or a short dated gap report when detail matters,
  and carries on; it owes no containment, checkpoint or repair.
- **Not a licence to skip the workflow.** Tracker keys remain the public
  identity, branches remain `{{ project.branch.pattern }}`, and the epic content
  contract still applies to every epic description.
- **Opt-in trials (new Paseo-direct plans only).** Milestone delivery
  (`delivery_strategy: milestone`) follows the
  [delivery milestones and adoption reference](../../create-implementation-plan/references/delivery-milestones-and-adoption.md)
  and the [checkout handoff controls](../SKILL.md#checkout-ownership-and-reuse).
  Behaviour scenarios (`behaviour_scenarios: bdd`) follow
  [Behaviour scenarios](../../create-implementation-plan/SKILL.md#behaviour-scenarios-opt-in-trial):
  opt-in, manual runs during the pilot, no CI change.
- **Retirement.** `{{ project.decision_authorities.platform }}` makes
  Kontor-governed the default again by editing this section in a dated commit
  once every `kontord` gap row in the ledger is resolved and one
  Kontor-governed epic has run from kickoff to closeout without adding a new
  one. No counter, streak or elapsed time changes the default on its own.
