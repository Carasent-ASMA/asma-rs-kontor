# {{ epic.key }} — {{ epic.title }}

> **Tracker:** {{ project.tracker.kind }} `{{ project.tracker.project_key }}`
> **Parent:** {{ epic.parent }}
> **Status:** {{ epic.status }}
> **Workstream:** {{ epic.workstream }}
> **Orchestration / workflow:** {{ epic.orchestration }} / {{ epic.workflow }}
> **Owner:** {{ project.owner }}

## Summary / purpose

{{ epic.summary }}

## Goal

{{ epic.goal }}

## Scope and sub-goals

{{ epic.scope }}

- {{ epic.sub_goal }}

## Done means

{{ epic.closure }}

The closure sentence above is copied verbatim from the authoritative plan. It
is testable: a reader can point at the evidence that proves it.

## Detailed plan

- Authoritative plan: [{{ epic.plan_title }}]({{ epic.plan_path }})
- Bound children: {{ epic.children }}
- Dependencies and external fences: {{ epic.dependencies }}
- Workstream: {{ epic.workstream }}
- Review and merge authority: {{ project.merge_authority.role }}
