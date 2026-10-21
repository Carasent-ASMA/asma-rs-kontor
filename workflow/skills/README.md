# Workflow skills

These skills are the coordination and planning part of the Kontor workflow
pack. They are generic: every value that changes by project, user or machine is
a parameter or a live read, not text in the skill.

This directory is the only editable copy of these skills. A consuming project
renders its own copy from a pinned pack revision; a rendered copy is never
hand-edited. A rule that moves into the pack is removed from the consuming
project's private copy, so one rule has one truth.

## Placeholder grammar

- `{{ project.<key> }}` is filled from the `project:` block in the selected
  config folder's `orchestration/orchestration.yml`. The neutral example is
  [`config/examples/orchestration/orchestration.yml`](../../config/examples/orchestration/orchestration.yml).
- A project-specific example of a value that is not a placeholder belongs in a
  fenced example block marked with `<!-- example: project-specific -->`.
  Nothing outside such a block may name a real project, person, account, host,
  tracker key or private repository path.
- An unfilled placeholder is an error. The renderer refuses it instead of
  shipping `{{ ... }}` into an instruction file.
- A live read is not a parameter. Tracker issue types, status names, transition
  ids, catalog revisions and installed runtime capabilities are read from the
  owning system every time. The skill states where to read them.

## Rule ownership

Each repeated rule has one owner. Other skills link to the owner and do not
restate its body. The ownership table is also checked by
`scripts/tests/test_workflow_pack_skills.py`.

| Rule | Owner | Linked from |
| --- | --- | --- |
| Orchestration surface and transition rule | [`epic-orchestration/references/orchestration-surface-transition.md#orchestration-surface-transition-rule`](epic-orchestration/references/orchestration-surface-transition.md#orchestration-surface-transition-rule) | mini-project-kickoff, agent-dispatch, seat-handoff, seat-takeover, ticket-triage-tiers, tracker-linked-creation, long-task-commit-checkpoints |
| Ambiguity and uncertainty | [`create-implementation-plan/SKILL.md#ambiguity-and-uncertainty`](create-implementation-plan/SKILL.md#ambiguity-and-uncertainty) | epic-orchestration, mini-project-kickoff, ticket-triage-tiers |
| Task sizing and grouping | [`create-implementation-plan/SKILL.md#task-sizing-and-grouping`](create-implementation-plan/SKILL.md#task-sizing-and-grouping) | mini-project-kickoff, ticket-triage-tiers, tracker-linked-creation |
| Epic content contract | [`tracker-linked-creation/SKILL.md#epic-content-contract`](tracker-linked-creation/SKILL.md#epic-content-contract) | epic-orchestration, mini-project-kickoff, create-implementation-plan |
| Task content contract | [`tracker-linked-creation/SKILL.md#task-content-contract`](tracker-linked-creation/SKILL.md#task-content-contract) | epic-orchestration, mini-project-kickoff, create-implementation-plan, long-task-commit-checkpoints |
| Public identity and naming | [`tracker-linked-creation/SKILL.md#tracker-public-identity-and-naming`](tracker-linked-creation/SKILL.md#tracker-public-identity-and-naming) | epic-orchestration, mini-project-kickoff, create-implementation-plan |
| First line is the exact seat name | [`agent-dispatch/SKILL.md#first-line-is-the-exact-seat-name`](agent-dispatch/SKILL.md#first-line-is-the-exact-seat-name) | seat-takeover |
| Common evidence envelope | [`seat-handoff/references/evidence-envelope.md`](seat-handoff/references/evidence-envelope.md) | seat-handoff, seat-takeover |
| Checkout ownership and collision safety | [`epic-orchestration/SKILL.md#checkout-ownership-and-reuse`](epic-orchestration/SKILL.md#checkout-ownership-and-reuse) | ticket-triage-tiers, long-task-commit-checkpoints |
| Capacity | [`epic-orchestration/references/paseo-direct-native-placement.md#capacity`](epic-orchestration/references/paseo-direct-native-placement.md#capacity) | epic-orchestration, mini-project-kickoff |
| Plan graph contract | [`create-implementation-plan/references/plan-graph-contract.md`](create-implementation-plan/references/plan-graph-contract.md) | epic-orchestration, mini-project-kickoff |
| Delivery milestones | [`create-implementation-plan/references/delivery-milestones-and-adoption.md`](create-implementation-plan/references/delivery-milestones-and-adoption.md) | epic-orchestration, long-task-commit-checkpoints |
| Behaviour scenarios | [`create-implementation-plan/references/behaviour-scenarios-opt-in-trial.md`](create-implementation-plan/references/behaviour-scenarios-opt-in-trial.md) | epic-orchestration, mini-project-kickoff, tracker-linked-creation |
| Triage tiers | [`ticket-triage-tiers/SKILL.md#tiers`](ticket-triage-tiers/SKILL.md#tiers) | mini-project-kickoff, agent-dispatch |
| Branch and commit grammar | [`long-task-commit-checkpoints/SKILL.md#the-mandated-lifecycle`](long-task-commit-checkpoints/SKILL.md#the-mandated-lifecycle) | epic-orchestration, create-implementation-plan |

The seat lifecycle is split by transition, not repeated: `agent-dispatch` owns
starting a seat, `seat-handoff` owns a phase transition or rework, and
`seat-takeover` owns resuming or replacing the same occupant. Each links to the
other two.

## Skills

| Skill | Use it for |
| --- | --- |
| [`epic-orchestration`](epic-orchestration/SKILL.md) | Drive an existing scoped epic from its confirmed task graph through delivery, gates and closeout. |
| [`mini-project-kickoff`](mini-project-kickoff/SKILL.md) | Turn an agreed initiative into one confirmed, executable backlog and handoff. |
| [`agent-dispatch`](agent-dispatch/SKILL.md) | Start or delegate a bounded request to a declared logical seat. |
| [`seat-handoff`](seat-handoff/SKILL.md) | Move ownership of the next authorized phase, or route rework and remediation. |
| [`seat-takeover`](seat-takeover/SKILL.md) | Resume an unfinished occupant or replace it with one verified successor. |
| [`ticket-triage-tiers`](ticket-triage-tiers/SKILL.md) | Classify risk and work profile before selecting a team or materializing a task. |
| [`create-implementation-plan`](create-implementation-plan/SKILL.md) | Write a durable, executable plan with an explicit ambiguity rule. |
| [`tracker-linked-creation`](tracker-linked-creation/SKILL.md) | Create, link or reconcile a tracker epic and its tasks under the content contracts. |
| [`long-task-commit-checkpoints`](long-task-commit-checkpoints/SKILL.md) | Branch, checkpoint, publish and land a long or multi-phase job. |

## Project parameters used here

The skills use only keys from the `project:` block. The renderer and the tests
check that every placeholder below resolves from the selected config folder.

| Parameter | Used for |
| --- | --- |
| `project.key`, `project.name`, `project.owner` | Public project identity and the responsible operator. |
| `project.decision_authorities.platform`, `project.decision_authorities.architecture`, `project.decision_authorities.decision_records_root` | Escalation and decision-record authority, never a named person in the skill. |
| `project.tracker.kind`, `project.tracker.project_key`, `project.tracker.site`, `project.tracker.key_pattern`, `project.tracker.credential_alias`, `project.tracker.spec` | Tracker scope, identity and the pinned field/workflow catalog. |
| `project.branch.pattern`, `project.branch.commit_pattern`, `project.branch.types` | Branch and commit grammar. |
| `project.forge.kind`, `project.forge.organization`, `project.forge.repositories`, `project.forge.base_branch` | Repository, organization and default branch. |
| `project.runtime.matrix` | Allowed provider/harness pairs. |
| `project.cli.command`, `project.cli.verbs` | The project CLI used for checkout, commits, publication and task commands. |
| `project.team_pack`, `project.fleet`, `project.supervision` | Team overlay, fleet policy and supervision policy. |
| `project.merge_authority.role`, `project.merge_authority.policy` | Who may merge and under which policy. |
| `project.documentation_roots`, `project.gap_ledger`, `project.glossary`, `project.attention_channel` | Documentation and gap routing. |
| `project.worktree_layout`, `project.check_runs` | Checkout isolation and required checks. |
| `project.capacity.max_delivery_seats`, `project.capacity.max_parallel_tasks` | Admission ceilings. |
| `project.time_zone`, `project.holiday_countries`, `project.currency` | Calendar and reporting facts. |
| `project.resource_roots.project` | Project resource path root. |

## Inventory rows fixed here

The source inventory assigns these rows to WP-03. Each row is fixed in the pack
copy named below, not carried as a stale fact.

| Row | Source skill | Disposition |
| --- | --- | --- |
| WP03-INV-01 | `epic-orchestration`, `orchestration-surface-transition` | Named decision authorities and personal escalation are replaced by `{{ project.decision_authorities.platform }}` and `{{ project.decision_authorities.architecture }}`; the retirement authority is the platform authority, not a person. |
| WP03-INV-02 | `epic-orchestration`, `paseo-direct-native-placement`, `seat-handoff` | The supervision file is read from the selected config folder as `{{ project.supervision }}`; the skill is not a mirror of an owner-local state file. |
| WP03-INV-03 | `agent-dispatch` | Host asset paths, account/keychain items and the real worktree example are removed; host values belong in `host.yml`, and examples use synthetic paths. |
| WP03-INV-04 | `mini-project-kickoff`, `tracker-linked-creation` | Tracker scope is `{{ project.tracker.project_key }}`; issue types, status names and transition ids are read live from the pinned tracker catalog, never copied into the skill. |
| WP03-INV-05 | `paseo-direct-native-placement` | Capacity 7/4 is replaced by `{{ project.capacity.max_delivery_seats }}` and `{{ project.capacity.max_parallel_tasks }}`. |
| WP03-INV-06 | `long-task-commit-checkpoints`, `create-implementation-plan` | Every `master` reference is `{{ project.forge.base_branch }}`; the branch grammar is `{{ project.branch.pattern }}` and the commit grammar is `{{ project.branch.commit_pattern }}`. |
| WP03-INV-07 | `seat-takeover` recovery reference | Installed-runtime version claims are removed; the operator rechecks the resolved runtime's supported operations at use time. |
| WP03-INV-08 | `epic-orchestration` | Stale status, count and clock facts are removed or linked to their live owner; the skill carries no dated project state. |

## Neutrality

The pack is public. It contains no private repository path, person, account,
cost, credential, host path or tracker key. A skill may describe a project
decision in neutral words and link to this repository's public
[`docs/WORKFLOW.md`](../../docs/WORKFLOW.md),
[`docs/GLOSSARY.md`](../../docs/GLOSSARY.md),
[`docs/PRINCIPLES.md`](../../docs/PRINCIPLES.md) and
[`docs/decisions/`](../../docs/decisions/README.md).
