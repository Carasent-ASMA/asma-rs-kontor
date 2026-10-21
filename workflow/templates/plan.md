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
> **Category:** plan
> **Scope:** {{ document.scope }}
> **Summary:** {{ document.summary }}

**Orchestration:** {{ document.orchestration }}

One workflow, two orchestration options: the direct option coordinates without
a daemon; the governed option routes orchestration through the daemon and adds
deterministic automation and enforcement. Both read the same selected config
folder.

## When to Load

**Load this document when:**

- {{ document.load_when }}

**Do NOT load for:** {{ document.not_for }}

## 0. Approval and authority

Record the decision that authorizes this plan, who accepted it, and which
tracker item binds it. Do not restate an approval that lives elsewhere; link the
owning record.

## 1. Requirements and constraints

| ID | Requirement or constraint | Verification |
| --- | --- | --- |
| REQ-001 | {{ document.requirement }} | {{ document.verification }} |
| CON-001 | {{ document.constraint }} | {{ document.verification }} |

The selected config folder is explicit:

- `--config-dir <path>` wins over `KONTOR_CONFIG_DIR`.
- A relative input is resolved once at setup and persisted as an absolute path.
- No consumer guesses the folder from the current directory, a state root or a
  legacy project path.
- A missing selection, missing folder, missing referenced file or invalid YAML
  is refused by name.

## 2. Implementation steps

| Task | Tracker key | Description | Closure condition | Completed | Date |
| --- | --- | --- | --- | --- | --- |
| TASK-001 | {{ task.key }} | {{ task.description }} | {{ task.closure }} | | |
| TASK-002 | {{ task.key }} | {{ task.description }} | {{ task.closure }} | | |

### 2.5 Dependency Graph and Waves

The graph is the only editable dependency source. The `jira` field is the
plan-graph v1 field name; it holds the confirmed tracker key for any tracker.

```plan-graph
{
  "schema_version": 1,
  "epic": {"jira": "{{ document.epic }}", "kontor_id": null},
  "tasks": [
    {"id": "TASK-001", "title": "{{ task.title }}", "phase": "Phase 1", "jira": "{{ task.key }}", "kontor_id": null, "depends_on": []},
    {"id": "TASK-002", "title": "{{ task.title }}", "phase": "Phase 1", "jira": "{{ task.key }}", "kontor_id": null, "depends_on": ["TASK-001"]}
  ],
  "external_fences": []
}
```

<!-- plan-graph:derived:start -->

```text
wave 0   TASK-001
wave 1   TASK-002
```

<!-- plan-graph:derived:end -->

## 3. Alternatives

| ID | Alternative | Why rejected |
| --- | --- | --- |
| ALT-001 | {{ document.alternative }} | {{ document.rejection_reason }} |

## 4. Dependencies and fences

| ID | Dependency | Owner | Lift condition |
| --- | --- | --- | --- |
| DEP-001 | {{ document.dependency }} | {{ project.owner }} | {{ document.lift_condition }} |

## 5. Files

- `{{ document.path }}` — {{ document.purpose }}

## 6. Testing

| Test | Proves |
| --- | --- |
| TEST-001 | {{ document.test_proof }} |

## 7. Risks and assumptions

| ID | Risk or assumption | Mitigation |
| --- | --- | --- |
| RISK-001 | {{ document.risk }} | {{ document.mitigation }} |

## 8. Holds

A plan with `status: On Hold` must carry exactly this block. The reason is from
the closed list; `review_by` is at most 30 days ahead and silence never extends
a hold.

```yaml
hold:
  reason: waiting_on_dependency   # waiting_on_dependency | waiting_on_capacity | waiting_on_decision | waiting_on_external_party | deprioritised
  owner: '{{ project.owner }}'
  lifts_when: '{{ document.lift_condition }}'
  review_by: '{{ document.review_by }}'
```

## 9. Decisions and open questions

| ID | Question or decision | Owner | Disposition |
| --- | --- | --- | --- |
| Q-01 | {{ document.question }} | {{ project.owner }} | {{ document.disposition }} |

## 10. Related documents

- [{{ document.related_title }}]({{ document.related_path }})
