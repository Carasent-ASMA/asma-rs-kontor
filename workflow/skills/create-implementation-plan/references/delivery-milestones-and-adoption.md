### Delivery milestones and adoption

A **Delivery Milestone** is an independently useful or risk-reducing increment
of the epic that is safely deployable and verifiable. It owns one PR per affected
repository, forming a coordinated PR set when it spans repositories.
It is a plan grouping, not a new tracker level, work-profile phase or runtime
object. Use plan-local labels such as `M1`; they never replace confirmed tracker
keys.

Choose the smallest useful delivery sequence. A single milestone is sufficient
when the epic is already reviewable. A single-repository, single-milestone epic
uses the existing branch/PR shape and can state the mapping in its existing task
table; no extra issue, workspace or document is needed. For each milestone, say
what remains usable if the epic stops after it, and specify compatibility,
acceptance and rollout.
An enabling increment needs independently verifiable value and safe behavior;
a pile of incomplete changes is not made releasable by naming it a milestone.
Prefer completing one milestone before starting the next; justify overlapping
milestones with their integration and compatibility costs.

For a new Paseo-direct plan selecting this trial, record
`delivery_strategy: milestone` and the milestone maps in the plan's §2.6 (add
the section when the selected template does not carry one). Task-sizing guidance
applies to new and substantively revised scope on both orchestration surfaces.
The branch rules for each delivery strategy are in
[Milestone branches and publication](#milestone-branches-and-publication).

Before scheduling, verify the branch/PR/integration operations against the
installed project CLI and the checkout/seat controls against the selected
runtime. Record unsupported operations and a workable fallback. If milestone
mechanics cannot be demonstrated, adopt task-sizing improvements with the
existing agreed branch/worktree flow and mark the advanced topology unadopted.
A serial alternative is eligible only within verified exclusive ownership; it
does not authorize cross-task checkout reuse or bypass the cross-task
reassignment restriction. Do not invent flags, silently replace a milestone
with many task PRs, or waive publication attestation. Branch mechanics
belong to
[long-task-commit-checkpoints](../../long-task-commit-checkpoints/SKILL.md#milestone-branches-and-publication);
checkout handoff belongs to
[epic-orchestration](../../epic-orchestration/SKILL.md#checkout-ownership-and-reuse).

#### Milestone branches and publication

Resolve the plan's delivery strategy before creating a branch.

- **New epic plan without milestone selection:** keep one epic integration
  branch keyed to the confirmed epic and one PR to
  `{{ project.forge.base_branch }}` per affected repository. Another delivery
  strategy requires an explicit plan decision; this default does not require an
  epic for standalone tasks.
- **Existing plans:** retain their agreed delivery strategy (for an epic, its
  epic branch/PR strategy) until an explicit plan amendment. A new skill
  version never migrates active work; never repartition live branches or
  tracker history automatically.
- **Paseo-direct milestone plan:** use one integration branch per milestone per
  repository, linked to the confirmed epic tracker key. Keep the canonical
  grammar `{{ project.branch.pattern }}`; the milestone label belongs in the
  slug and plan, never in place of the tracker key. Each repository PR names
  its milestone scope and included task keys; merging one milestone does not
  close the whole epic.
- **Standalone task:** use its confirmed task key. Do not create an epic solely
  to collect unrelated small changes.
- **Kontor-governed work:** preserve the deployed bindings (including the
  task-worktree binding), gates, completion profile and publication/completion
  contracts. Milestone intent in a document cannot install unsupported runtime
  semantics, and does not authorize hand-built branch names, arbitrary PR bases
  or changed immutable pins.

Create the next milestone from the updated default branch after the preceding
milestone merges; do not continue a squash-merged branch as the next
milestone's history.

Parallel contribution branches may integrate into the named milestone branch
only through a verified supported integration operation. Record contributor
SHAs, resolved integration HEAD and the tasks they implement. One integrator
owns that branch; contributors do not publish their partial work to the default
branch just because they finished a task.

Validate and review the combined milestone HEAD before publication/merge.
A PR set spanning repositories declares compatibility and merge/deployment
order, including recovery when only part of the set succeeds; cross-repository
merges are not atomic. Keep implementation, integration, merge and release
observations separate. Move the tracker per the task's actual Done means through
the owning workflow; one shared epic key or PR event never closes unrelated
tasks.

An urgent independent fix branches from the supported released/default base
with its own confirmed issue key. After its normal review and release, reconcile
it into unfinished milestone work through supported operations and rerun the
affected checks; never publish unfinished milestone scope with the urgent fix.

#### Orchestrating a milestone

- The TPM routes the plan's task, milestone and checkout maps; the LSA owns
  scope and architectural changes.
- Before each milestone, confirm its scope, prerequisites, repository targets,
  integration owner and usable stopping point.
- Integration belongs to the existing authorized TPM or operator, not a new
  seat; code conflicts go back to the responsible delivery seat. A later change
  invalidates affected evidence; resolve conflicts before claiming integration.
- A milestone adds no native seat. Keep existing TSW and role identities and
  risk-based reviews; admit only useful ready work with exclusive checkouts.
- Apply the existing review, Committee and release gates to the milestone's
  actual scope and risk. A trial never waives a gate required before publication.
- Implemented, integrated, merged and released are observations, not new
  tracker or `kontord` lifecycle values. Final epic closure still verifies the
  whole goal, cross-milestone integration and every completion requirement.
- On a scope change, update the plan's mappings and acceptance evidence before
  redispatch and keep confirmed issue identities. Repeated tiny additions or an
  unreviewably large milestone trigger a sizing review. Use existing plan and PR
  evidence, not a second progress ledger.
