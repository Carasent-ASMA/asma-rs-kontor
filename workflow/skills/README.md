# Workflow skills

These skills are the workflow pack: coordination and planning, review and
committees, memory, and documentation. They are generic: every value that
changes by project, user or machine is a parameter or a live read, not text in
the skill.

This directory is the only editable copy of these skills. A consuming project
renders its own copy from a pinned pack revision; a rendered copy is never
hand-edited. A rule that moves into the pack is removed from the consuming
project's private copy, so one rule has one truth.

## Placeholder grammar

- `{{ project.<key> }}` is filled from the `project:` block in the selected
  config folder's `orchestration/orchestration.yml`. The neutral example is
  [`config/examples/orchestration/orchestration.yml`](../../config/examples/orchestration/orchestration.yml).
- A project-specific example of a value that is not a placeholder belongs in a
  fenced block opened by `<!-- example: project-specific -->` and closed by
  `<!-- /example: project-specific -->`. Nothing outside such a block may name
  a real project, person, account, host, tracker key or private repository
  path.
- An unfilled placeholder is an error. A renderer must refuse it instead of
  shipping `{{ ... }}` into an instruction file; WP-05 supplies that renderer.
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
| First line is the exact seat name | [`agent-dispatch/SKILL.md#first-line-is-the-exact-seat-name`](agent-dispatch/SKILL.md#first-line-is-the-exact-seat-name) | seat-takeover, epic-orchestration/references/paseo-direct-native-placement.md |
| Tracker convergence | [`tracker-linked-creation/SKILL.md#tracker-convergence`](tracker-linked-creation/SKILL.md#tracker-convergence) | epic-orchestration/references/kontor-governed.md |
| Common evidence envelope | [`seat-handoff/references/evidence-envelope.md`](seat-handoff/references/evidence-envelope.md) | seat-handoff, seat-takeover |
| Checkout ownership and collision safety | [`epic-orchestration/SKILL.md#checkout-ownership-and-reuse`](epic-orchestration/SKILL.md#checkout-ownership-and-reuse) | ticket-triage-tiers, long-task-commit-checkpoints |
| Capacity | [`epic-orchestration/references/paseo-direct-native-placement.md#capacity`](epic-orchestration/references/paseo-direct-native-placement.md#capacity) | epic-orchestration, mini-project-kickoff |
| Plan graph contract | [`create-implementation-plan/references/plan-graph-contract.md`](create-implementation-plan/references/plan-graph-contract.md) | epic-orchestration, mini-project-kickoff |
| Delivery milestones | [`create-implementation-plan/references/delivery-milestones-and-adoption.md`](create-implementation-plan/references/delivery-milestones-and-adoption.md) | epic-orchestration, long-task-commit-checkpoints |
| Behaviour scenarios | [`create-implementation-plan/references/behaviour-scenarios-opt-in-trial.md`](create-implementation-plan/references/behaviour-scenarios-opt-in-trial.md) | epic-orchestration, mini-project-kickoff, tracker-linked-creation |
| Triage tiers | [`ticket-triage-tiers/SKILL.md#tiers`](ticket-triage-tiers/SKILL.md#tiers) | mini-project-kickoff, agent-dispatch |
| Branch and PR-title grammar | [`long-task-commit-checkpoints/SKILL.md#the-mandated-lifecycle`](long-task-commit-checkpoints/SKILL.md#the-mandated-lifecycle) | epic-orchestration, create-implementation-plan |
| Communication contract | [`kontor-communication/SKILL.md`](kontor-communication/SKILL.md) | epic-orchestration, agent-dispatch, tracker-linked-creation, pr-review |
| Committee independence and a fresh judge | [`committee-protocol/SKILL.md`](committee-protocol/SKILL.md) | epic-orchestration, agent-dispatch, pr-review |
| Three homes for information | [`document-organization/SKILL.md`](document-organization/SKILL.md) | epic-orchestration, agent-dispatch, pr-review, docs-navigation, decision-records |
| Decision-record process | [`decision-records/SKILL.md`](decision-records/SKILL.md) | document-organization, docs-navigation |
| Mutation validation | [`mutation-testing/SKILL.md`](mutation-testing/SKILL.md) | create-implementation-plan |
| Experience memory | [`experience-memory/SKILL.md`](experience-memory/SKILL.md) | epic-orchestration, mini-project-kickoff/references/kontor-governed.md |
| Seat cards and role crosswalk | [`seat-cards/SKILL.md`](seat-cards/SKILL.md) | agent-dispatch, pr-review, committee-protocol |
| Review checklist and Done means | [`pr-review/references/review-checklist.md`](pr-review/references/review-checklist.md) | pr-review, pr-review/references/desk-messages.md |
| PR review labels and finding format | [`pr-review/references/comment-format.md`](pr-review/references/comment-format.md) | kontor-communication |
| No package or hook installs | [`seat-cards/SKILL.md#common-tool-boundary`](seat-cards/SKILL.md#common-tool-boundary) | pr-review, pr-review/references/desk-messages.md |

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
| [`kontor-communication`](kontor-communication/SKILL.md) | Write comments, notices, reviews, fix requests, status messages and handoffs. |
| [`pr-review`](pr-review/SKILL.md) | Review, re-review, fix and merge a pull request through the PR review desk. |
| [`committee-protocol`](committee-protocol/SKILL.md) | Run a formal committee with read-only seats, contrasting reviewers and a fresh judge. |
| [`seat-cards`](seat-cards/SKILL.md) | Brief a role from its card and map role code, fleet slot, seed role and seat label. |
| [`experience-memory`](experience-memory/SKILL.md) | Propose and verify an evidence-backed experience from settled work. |
| [`document-organization`](document-organization/SKILL.md) | Choose a documentation root, filename, metadata and router updates. |
| [`docs-navigation`](docs-navigation/SKILL.md) | Find and assess existing documentation through its routers and registers. |
| [`decision-records`](decision-records/SKILL.md) | Author, govern and retire decision records. |
| [`mutation-testing`](mutation-testing/SKILL.md) | Check whether tests detect seeded defects. |
| [`ubiquitous-language`](ubiquitous-language/SKILL.md) | Extract and maintain a domain glossary. |

## Project parameters used here

The skills use only keys from the `project:` block. The tests check that every
placeholder below resolves from the selected config folder; the WP-05 renderer
must apply the same rule.

| Parameter | Used for |
| --- | --- |
| `project.key`, `project.name`, `project.owner` | Public project identity and the responsible operator. |
| `project.decision_authorities.platform`, `project.decision_authorities.architecture`, `project.decision_authorities.decision_records_root` | Escalation and decision-record authority, never a named person in the skill. |
| `project.tracker.kind`, `project.tracker.project_key`, `project.tracker.site`, `project.tracker.key_pattern`, `project.tracker.credential_alias`, `project.tracker.spec` | Tracker scope, identity and the pinned field/workflow catalog. |
| `project.branch.pattern`, `project.branch.commit_pattern`, `project.branch.types` | Branch grammar and the PR-title/squash-subject grammar; checkpoint subjects follow the project CLI's own validated convention. |
| `project.forge.kind`, `project.forge.organization`, `project.forge.repositories`, `project.forge.base_branch` | Repository, organization and default branch. |
| `project.runtime.matrix` | Allowed provider/harness pairs. |
| `project.cli.command`, `project.cli.verbs` | The project CLI used for checkout, commits, publication and task commands. |
| `project.team_pack`, `project.fleet`, `project.supervision` | Team overlay, fleet policy and the autonomous TPM cadence profile. |
| `project.merge_authority.role`, `project.merge_authority.policy` | Who may merge and under which policy. |
| `project.documentation_roots`, `project.gap_ledger`, `project.glossary`, `project.attention_channel` | Documentation and gap routing. |
| `project.worktree_layout`, `project.check_runs` | Checkout isolation and required checks. |
| `project.capacity.max_parallel_tasks`, `project.capacity.start_admission`, `project.capacity.max_delivery_seats` | Team-run placement ceiling, initial admission, and the separate owner-set per-team-run value. |
| `project.time_zone`, `project.holiday_countries`, `project.currency` | Calendar and reporting facts. |
| `project.resource_roots.project` | Project resource path root. |

## Inventory rows fixed here

The source inventory assigns these rows to WP-03 and WP-04. Each row is fixed
in the pack copy named below, not carried as a stale fact.

| Row | Source skill | Disposition |
| --- | --- | --- |
| WP03-INV-01 | `epic-orchestration`, `orchestration-surface-transition` | Project-specific named authorities and personal escalation wording are replaced by `{{ project.decision_authorities.platform }}`; the parameter value names the platform authority, and the skill names no person. The architecture/ADR authority part belongs to WP-04. |
| WP03-INV-02 | `epic-orchestration`, `paseo-direct-native-placement`, `seat-handoff` | The TPM cadence profile is `{{ project.supervision }}`; the Paseo watchdog/recovery policy is `<state-root>/supervision.yml`, sampled by `config/examples/paseo-supervision.yml`. One key does not stand for both files, and neither is a mirror of an owner-local state file. |
| WP03-INV-03 | `agent-dispatch` | Host asset paths, account/keychain items and the real worktree example are removed; host values belong in `host.yml`, and examples use synthetic paths. |
| WP03-INV-04 | `mini-project-kickoff`, `tracker-linked-creation` | Tracker scope is `{{ project.tracker.project_key }}`; issue types, status names and transition ids are read live from the pinned tracker catalog, never copied into the skill. |
| WP03-INV-05 | `paseo-direct-native-placement` | Capacity 7/4 becomes `{{ project.capacity.max_parallel_tasks }}` (ceiling) and `{{ project.capacity.start_admission }}` (initial admission). `{{ project.capacity.max_delivery_seats }}` is a separate owner-set per-team-run value, not team-run admission. |
| WP03-INV-06 | `long-task-commit-checkpoints`, `create-implementation-plan` | Base-branch references that named `master` now resolve from `{{ project.forge.base_branch }}`; branch grammar resolves from `{{ project.branch.pattern }}`, and `{{ project.branch.commit_pattern }}` governs the PR title/squash subject. Checkpoint commit subjects follow the project CLI's own validated convention, read live from the CLI, not `commit_pattern`. |
| WP03-INV-07 | `seat-takeover` recovery reference | Installed-runtime version claims are removed; the operator rechecks the resolved runtime's supported operations at use time. |
| WP03-INV-08 | `epic-orchestration` | Stale status, count and clock facts are removed or linked to their live owner; the skill carries no dated project state. |

The source inventory assigns these rows to WP-04. The WP-03 rows above stay
unchanged.

| WP-04 row | Source skill | Disposition |
| --- | --- | --- |
| WP04-INV-01 | `decision-records`, `epic-orchestration`, `orchestration-surface-transition`, `pr-review` | Named decision authorities, personal escalation wording and personal merge wording are replaced by `{{ project.decision_authorities.architecture }}`, `{{ project.merge_authority.role }}` and `{{ project.owner }}`. The pack names no person. |
| WP04-INV-02 | `pr-review`, `seat-cards` | The learning log and every desk history path use the desk's host-configured folder, never an absolute home path. The desk folder stays a host value. |
| WP04-INV-03 | `pr-review`, `tracker-linked-creation`, `mini-project-kickoff` | Tracker scope is `{{ project.tracker.project_key }}`; issue types, status names and transition ids are read live from the pinned tracker catalog. The PR review skill names no project key. |
| WP04-INV-04 | `pr-review`, `epic-orchestration` | Capacity stays owned by the placement rule (`{{ project.capacity.max_parallel_tasks }}`, `{{ project.capacity.start_admission }}`); PR desk limits are read from the project's desk settings, not copied into the skill. |
| WP04-INV-05 | `pr-review`, `long-task-commit-checkpoints`, `create-implementation-plan` | Base-branch references resolve from `{{ project.forge.base_branch }}`; branch grammar resolves from `{{ project.branch.pattern }}`. The PR review skill names no default branch. |
| WP04-INV-06 | `pr-review` | Organization scope is `{{ project.forge.organization }}`; the runtime port is a fact read from the runtime, not a copied project value. |
| WP04-INV-07 | `experience-memory`, `pr-review` | The review checklist owns 11 checks including Done means; the judge card, the comment format and the memory skill refer to check (11) instead of keeping a second count or a dated time. |

## Neutrality

The pack is public. It contains no private repository path, person, account,
cost, credential, host path or tracker key. A skill may describe a project
decision in neutral words and link to this repository's public
[`docs/WORKFLOW.md`](../../docs/WORKFLOW.md),
[`docs/GLOSSARY.md`](../../docs/GLOSSARY.md),
[`docs/PRINCIPLES.md`](../../docs/PRINCIPLES.md) and
[`docs/decisions/`](../../docs/decisions/README.md).
