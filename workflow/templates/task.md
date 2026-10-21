# {{ task.key }} — {{ task.title }}

> **Tracker:** {{ project.tracker.kind }} `{{ project.tracker.project_key }}`
> **Parent epic:** {{ task.parent }}
> **Tier:** {{ task.tier }}
> **Status:** {{ task.status }}
> **Owner:** {{ task.owner }}

| Field | Value |
| --- | --- |
| Dependencies | {{ task.dependencies }} |
| External fences | {{ task.fences }} |
| File ownership | {{ task.file_ownership }} |
| Publication | {{ task.publication }} |
| Inventory rows | {{ task.inventory_rows }} |

## Summary / purpose

{{ task.summary }}

## Goal

{{ task.goal }}

## Scope and sub-goals

{{ task.scope }}

- {{ task.sub_goal }}

## Delivery context

{{ task.delivery_context }}

- Primary module or repository: {{ task.module }}
- Affected paths: {{ task.paths }}
- Checkout, branch and base: `{{ project.branch.pattern }}`,
  `{{ project.forge.base_branch }}`
- Config folder: explicitly selected; relative references resolve from its root.

## Verification and evidence

{{ task.verification }}

- Commands run and exact results: {{ task.commands }}
- Tests and checks: {{ task.checks }}
- Evidence location: {{ task.evidence }}

## Detailed plan

- Authoritative plan: [{{ task.plan_title }}]({{ task.plan_path }})
- Plan task id: {{ task.plan_id }}

## Done means

{{ task.closure }}

The closure sentence above is copied verbatim from the authoritative plan. A
task closes only when its assigned inventory rows have recorded dispositions and
its required evidence is attached.
