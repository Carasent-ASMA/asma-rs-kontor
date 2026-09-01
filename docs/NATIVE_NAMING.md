# Native naming

> Status: approved contract; local implementation verified, live epic migration pending.

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

The v1 schema, recommended ASMA fixture, publication/selection API and resumable
epic migration implement this contract. Existing topology-v4 centered-dot names
remain historical evidence until their owning epic completes an explicit Team
Definition upgrade.

An epic may freeze its Team Definition at logical creation before Jira
materialization completes. That pin does not authorize a native effect: ESW,
ECP, TSW, ASW, CSW and seat materialization fail closed until the epic has an
active immutable backlog code and the addressed epic/task has one exact
confirmed Jira readback.

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

This is the implemented snake-case v1 wire shape. Templates are typed segment
vectors; the configured separator is inserted between rendered segments.

```json
{
  "schema_version": 1,
  "definition_id": "01936f5a-2000-7000-8000-000000000001",
  "version": 1,
  "name": "ASMA Operational Team Definition",
  "topology": { "spec_id": "...", "version": 4, "canonical_hash": "..." },
  "separator": " • ",
  "containers": [
    {
      "kind": "ESW",
      "parent": null,
      "prefix": "ESW",
      "projection_capabilities": ["native_root"],
      "read_only": false,
      "name_template": { "segments": [
        { "kind": "token", "value": "PREFIX" },
        { "kind": "token", "value": "EPIC_ITEM_CODE" }
      ] }
    },
    {
      "kind": "ASW",
      "parent": "ESW",
      "prefix": "ASW",
      "projection_capabilities": ["native_child", "session_host"],
      "read_only": true,
      "name_template": { "segments": [
        { "kind": "token", "value": "PREFIX" },
        { "kind": "token", "value": "SCOPE_ITEM_CODE" },
        { "kind": "token", "value": "TOPIC" }
      ] },
      "seat_name_template": { "segments": [
        { "kind": "token", "value": "ROLE_CODE" }
      ] },
      "slots": [
        { "slot_id": "software-architect", "role_code": "SA", "capability_profile": "independent-advisor" },
        { "slot_id": "auditor", "role_code": "AUD", "capability_profile": "independent-advisor" }
      ]
    }
  ]
}
```

The shipped document also declares ECP (`LSA`, `TPM`), TSW (role-coded,
team-template supplied delivery slots) and CSW (`SEAT A`, `SEAT B`, `JUDGE`)
rows. The complete canonical fixture is
`crates/kontor-profiles/fixtures/operational-domain.json`.

## Publication and migration

- Validate and publish immutable revisions with
  `team-definitions:validate` and `team-definitions:publish`.
- Change what future epics inherit with the project
  `team-definition-selection:preview` / `:apply` compare-and-swap.
- Move an existing epic only through
  `team-definition:upgrade-preview` / `:upgrade-apply`.
- A migration preview binds every container and seat to its full native runtime
  identity, parent, kind, cwd, observed title and desired title.
- Legacy ASW/CSW topics are explicit operator input keyed by topology-node id;
  unknown, missing or extra mappings are refused.
- Apply records its intent before the first external retitle. Partial effects
  leave the old pin in force and the epic fenced. The same idempotency key
  resumes from exact readback; a different key cannot interleave.
- The pin switches only after every target reads back the desired title under
  the unchanged native identity. Backup/export includes definitions, defaults,
  pins, migration intents, targets, topic provenance and per-seat advice.

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
