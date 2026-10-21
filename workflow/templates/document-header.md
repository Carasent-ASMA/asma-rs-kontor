---
goal: '{{ document.goal }}'
version: '{{ document.version }}'
date_created: '{{ document.date_created }}'
last_updated: '{{ document.last_updated }}'
status: '{{ document.status }}'
epic: '{{ document.epic }}'
workstream: '{{ document.workstream }}'
orchestration: '{{ document.orchestration }}'
workflow: '{{ document.workflow }}'
tags: [{{ document.tags }}]
---

# {{ document.title }}

> **Date:** {{ document.date }} {{ project.time_zone }}
> **Status:** {{ document.status }} — {{ document.status_note }}
> **Category:** {{ document.category }}
> **Scope:** {{ document.scope }}
> **Summary:** {{ document.summary }}

## When to Load

**Load this document when:**

- {{ document.load_when }}

**Do NOT load for:** {{ document.not_for }}

## Context

{{ document.context }}

## Decision or finding

{{ document.decision }}

## Evidence

{{ document.evidence }}

## Open questions

{{ document.open_questions }}

## Related documents

- [{{ document.related_title }}]({{ document.related_path }})
