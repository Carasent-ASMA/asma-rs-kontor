# Native naming

> Status: approved contract; implemented locally, with final archive
> verification, independent audit and live epic migration pending.

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

Quota-driven succession does not create a naming exception. A successor keeps
the same immutable TeamRun role slot and is rendered again from that slot's
exact pinned Team Definition mapping. Provider, account, rung, predecessor id
and Jira key remain authority/evidence fields; none is appended to the native
seat title. For example, the successor of an `SA` seat is still named `SA`, not
an account-, rung-, Jira- or predecessor-decorated variant.

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

The shipped document also declares ECP (`LSA`, `TPM`), TSW and CSW (`SEAT A`,
`SEAT B`, `JUDGE`) rows. ECP uses the exact deterministic Core Team slot
addresses `lsa→LSA` and `tpm→TPM`; their native titles remain the configured
uppercase role codes. TSW distinguishes fixed local `slots` from delivery
`team_slots`. The recommended revision registers exactly:

```json
"team_slots": [
  { "slot_id": "scope", "role_code": "SA", "capability_profile": "delivery-standard" },
  { "slot_id": "implement", "role_code": "SWE", "capability_profile": "delivery-standard" },
  { "slot_id": "verify", "role_code": "QA", "capability_profile": "delivery-standard" },
  { "slot_id": "audit", "role_code": "AUD", "capability_profile": "delivery-high" }
]
```

The catalog may register alternative-template slot ids under the same role code;
those alternatives do not necessarily coexist. Before scheduling or touching a
runtime, Kontor resolves the exact ordered slots of the frozen TeamTemplate.
Missing mappings, display-label mappings under the recommended `ROLE_CODE` TSW,
unknown role codes, or two slots in that one TeamRun rendering the same code are
`placement_blocked`. The shipped Research Spike slots remain deliberately
unregistered because `researcher-a` and `researcher-b` would both render `BA`;
they require a future `SLOT_DISPLAY_NAME` Team Definition revision rather than
an inferred suffix.

One exact `(container kind, RoleSlotId)` resolver covers both fixed local
`slots` and TeamRun-supplied `team_slots`. Its configured `role_code` or
`display_name` is the only rendering input. The older Operational
`delivery.role_bindings`, the TeamTemplate's logical role, a persisted
SeatBinding role and any caller-supplied role remain catalog or historical
placement evidence; they never override the exact governing Team Definition
during launch, replacement, reconciliation or migration. A missing exact slot
is `placement_blocked`; there is no role fallback.

The complete canonical fixture is
`crates/kontor-profiles/fixtures/operational-domain.json`.

## Publication and migration

- Validate and publish immutable revisions with
  `team-definitions:validate` and `team-definitions:publish`.
- Change what future epics inherit with the project
  `team-definition-selection:preview` / `:apply` compare-and-swap.
- Move an existing epic only through
  `team-definition:upgrade-preview` / `:upgrade-apply`.
- A migration preview binds every **active** container and seat to its full
  native runtime identity, parent, kind, cwd, observed title and desired title.
  Retired or archived nodes and inactive seats are immutable historical
  evidence: they are excluded from both the preview and persistence census,
  and their native titles are never rewritten to resemble the new definition.
- Reconcile lifecycle before preview through the supported settle, seat-retire,
  node-retire and node-archive surfaces. A runtime-archived historical workspace
  must be non-active in Kontor; never retire active work merely to bypass a
  migration refusal.
- Legacy ASW/CSW topics are explicit operator input keyed by topology-node id;
  unknown, missing or extra mappings are refused.
- Before upgrading a legacy epic, replay each ticket's existing
  `topology:materialize` command (or apply one stable repair key) to reconcile
  every open TeamRun slot into an exact logical SeatBinding. This logical repair
  creates no native session and is inert on repeated same-key replay.
- Migration preview requires every live bound delivery AgentRun to match exactly
  one active SeatBinding at the same `RoleSlotId`. A missing, duplicate or
  cross-slot binding refuses the complete census instead of omitting a seat.
- Before its first runtime read, upgrade preview resolves every exact slot of
  every open TeamRun hosted by an **active** topology node against the target
  Team Definition, including duplicate rendered-name checks for the slots that
  actually coexist in that run. An open run whose exact seats and node are
  inactive remains historical evidence and is not carried across the pin.
- The recorded and confirming censuses are bidirectional over both subject and
  immutable native identity: every active live pair must be enumerated exactly,
  and an extra, stale or identity-mismatched target refuses the migration.
- Apply records its intent before the first external retitle. Partial effects
  leave the old pin in force and the epic fenced. The same idempotency key
  resumes from fresh exact readback of every target; an earlier success that
  drifted is repaired again or remains pending. A different key cannot
  interleave.
- While that fence exists, delivery admission, topology materialization,
  replacement, seat release and every topology-node lifecycle transition
  refuse before writing a command, retiring a predecessor, changing logical
  lifecycle, creating a successor or contacting a runtime. The persistence
  fence shares the same immediate transaction as a seat release or node
  transition, so a migration census and a transition cannot race: whichever
  commits first determines whether the native is live or immutable history.
- The pin switches only after every target reads back the desired title under
  the unchanged native identity. Backup/export includes definitions, defaults,
  pins, migration intents, targets, topic provenance and per-seat advice.
- Schema v80 records the exact canonical apply intent. A data-bearing v79
  migration is backfilled only from its bound `upgrade_team_definition`
  command receipt. A recorded, applying or confirmed v79 migration without
  that receipt remains durable but is explicitly `legacy_unrecoverable`; it is
  fenced with a typed conflict and is never guessed from its fingerprint or
  target set.
- Supported generation-2 and generation-3 exports omit the generation-4 Team
  Definition arrays in their signed canonical representation. The reader adds
  empty arrays only to its in-memory current record type; legacy hashing,
  continuity and reserialization retain the exact source-generation shape.

## Validation and revision rules

- Missing or ambiguous prefix, template, parent, separator, role code, slot
  label, scope item code, topic or capability binding fails before runtime
  mutation.
- Delivery-slot placement is checked from durable configuration before runtime
  evidence is requested. A batch may still admit unrelated valid candidates;
  unnameable candidates remain individually `placement_blocked`.
- No fallback uses a UUID, title, Jira key, legacy task short code or
  caller-built string.
- Display names never establish identity; exact native IDs and bindings do.
- A naming change publishes a new immutable Team Definition revision.
- Existing runs keep their pin until an explicit preview/apply upgrade succeeds.
- Old receipts and evidence preserve their literal names and never become
  current templates.
- Historical `ADVICE`, fixed `ADVISOR`, `ASW · ...`, `Advisor · ...` and
  one-ASW/one-seat literals remain readable evidence only.
