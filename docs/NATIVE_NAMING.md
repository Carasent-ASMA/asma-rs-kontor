# Native naming

> Status: approved naming contract; implementation conformance audit pending.

Kontor must render native hierarchy and names deterministically from one
immutable, versioned Team Definition JSON revision pinned by the run. The Team
Definition owns container parents, prefixes, the literal separator, container
and seat templates, scope tokens, stable role codes, exact slot labels, slot
ordering, capabilities and placement policy.

`ProjectSessionTopologySpec` validates structural legality and runtime
projection capabilities. It is not a second naming authority. The role catalog
defines the meaning of a role code. It does not render a second seat name.
Models, clients and runtime adapters request semantic kinds/slots and never
construct a complete native name.

This document records the agreed target contract. It does **not** assert that
the current schema, seed packs, migrations, APIs, tests or live native objects
already conform. Those surfaces require the separately planned implementation
conformance audit. Existing topology-v4 centered-dot names remain historical
implementation and runtime evidence until an explicit versioned migration is
implemented and applied.

## Identity inputs

Jira keys remain full standard keys such as `ASMA-8001` and `ASMA-7869`. The
Jira project key is configured at Kontor project level, and each confirmed full
key remains persisted with connector evidence.

Every epic has one immutable epic backlog code that is case-insensitively unique
inside the Kontor project. Automatic allocation:

1. starts with uppercase title-word initials;
2. on collision appends the second character of word one, the second character
   of word two, and so on, then the third character of each word, continuing
   column-major until the first unique candidate;
3. after all title characters are exhausted, appends the smallest available
   integer starting at `2`.

A manual override is allowed only when valid and unique. `KOP` is the agreed
explicit code in these examples.

Item codes are derived projections:

```text
epic item code = <EPIC_BACKLOG_CODE>-<JIRA_EPIC_NUMBER>
task item code = <EPIC_BACKLOG_CODE>-<JIRA_TASK_NUMBER>
```

For example, `ASMA-8001` becomes `KOP-8001` and child `ASMA-7869`
becomes `KOP-7869`. The item code is not persisted as a second Jira binding and
is never parsed to reconstruct a full Jira key.

## Recommended Team Definition values

The literal separator is space + U+2022 BULLET + space: ` • `.

| Native object | Template | Example |
| --- | --- | --- |
| Paseo epic project / ESW | `ESW • <EPIC_ITEM_CODE>` | `ESW • KOP-8001` |
| ECP | `ECP • <EPIC_ITEM_CODE>` | `ECP • KOP-8001` |
| TSW | `TSW • <TASK_ITEM_CODE>` | `TSW • KOP-7869` |
| ASW | `ASW • <SCOPE_ITEM_CODE> • <TOPIC>` | `ASW • KOP-8001 • Jira recovery` |
| task CSW | `CSW • <TASK_ITEM_CODE> • <TOPIC>` | `CSW • KOP-7869 • Naming contract` |
| epic CSW | `CSW • <EPIC_ITEM_CODE> • <TOPIC>` | `CSW • KOP-8001 • Release readiness` |

ASW and CSW scopes follow the advised/debated subject, not the caller. A
task-specific subject uses the task item code; an epic-wide subject uses the
epic item code. An epic-global CSW remains a Committee workspace and is not an
Advisor workspace.

One ESW may contain zero or more ASWs. Each ASW represents one advised
subject/topic and contains one or more independently reporting advisor seats.
Follow-up rounds for the same consultation reuse that ASW and its seats; a
materially different subject or topic creates another ASW. An ASW has no Judge,
quorum, voting or aggregate verdict. Formal deliberation and aggregation belong
in a CSW. Do not create one global epic ASW and move the subject or topic into
its seat names; a UI may group an epic's ASWs without changing native identity.

The container carries scope and topic. Seats use only their configured local
role code or local slot label:

| Container | Recommended seat names |
| --- | --- |
| ECP | `LSA`, `TPM` |
| TSW | exact registered delivery role code, for example `AUD` |
| ASW | exact configured registered professional role or advisor-profile code, for example `SA` or `AUD` |
| Independent Review CSW | `SEAT A`, `SEAT B`, `JUDGE` |

Do not append an item code, Jira key, container prefix or topic to a seat name.
`ADVISOR` is not a universal native seat name. If the same advisor role occurs
more than once, the pinned Team Definition supplies exact distinct slot labels;
Kontor never invents suffixes.
Committee cardinality remains template-defined; these three labels are the
recommended Independent Review setup, not a universal kernel law.

## Recommended definition shape

Field names below express the approved semantic contract. The implementation
audit must decide how the current persisted schemas evolve to carry it.

```json
{
  "schemaVersion": 1,
  "naming": {
    "separator": " • ",
    "containers": {
      "esw": { "parent": null, "prefix": "ESW", "template": "{prefix}{separator}{epic.itemCode}" },
      "ecp": { "parent": "esw", "prefix": "ECP", "template": "{prefix}{separator}{epic.itemCode}" },
      "tsw": { "parent": "esw", "prefix": "TSW", "template": "{prefix}{separator}{task.itemCode}" },
      "asw": { "parent": "esw", "prefix": "ASW", "template": "{prefix}{separator}{scope.itemCode}{separator}{topic}" },
      "csw": { "parent": "esw", "prefix": "CSW", "template": "{prefix}{separator}{scope.itemCode}{separator}{topic}" }
    },
    "seatTemplates": {
      "leadership": "{role.code}",
      "delivery": "{role.code}",
      "advisor": "{role.code}",
      "committee": "{slot.displayName}"
    }
  }
}
```

## Validation and revision rules

- Missing or ambiguous prefix, template, parent, separator, role code, slot
  label, scope item code, topic or capability binding fails before runtime
  mutation.
- No fallback uses a UUID, title, Jira key, legacy task short code or
  caller-built string.
- Display names never establish identity; exact native IDs and bindings do.
- A naming change publishes a new immutable Team Definition revision.
- Existing runs keep their pin until an explicit preview/apply upgrade succeeds.
- Old receipts and evidence preserve their literal names and never become
  current templates.
- Historical `ADVICE`, fixed `ADVISOR`, `ASW · ...`, `Advisor · ...` and
  one-ASW/one-seat literals remain readable evidence only.
