---
goal: Make one immutable pinned Team Definition JSON revision the sole authority for native hierarchy and naming, then migrate the live KBI epic without changing native identities
version: 0.1
date_created: 2026-09-01
last_updated: 2026-09-01
owner: Kontor Lead Software Architect
status: In progress
tags: [kontor, naming, team-definition, topology, migration, ASMA-8062]
---

# Configuration-Driven Native Naming and Live Migration Plan

![Status: In progress](https://img.shields.io/badge/status-In%20progress-yellow)

> **Jira:** `ASMA-8062`
> **Kontor task:** `01a05d9b-8014-7182-9b96-921fc9386900` (`KBI-8062`)
> **Kontor epic:** `01a0539a-51c9-7301-9bd7-26c09167b23e` (`ASMA-8049`, backlog code `KBI`)
> **Runtime:** high-stakes TeamRun `01a05e46-f96a-7a00-b081-ed34b484a172` in TSW `wks_0f9f3ec5f36fd216`
> **Scope:** `_tools/asma-rs-kontor`, its public contracts and repository documentation, plus the authoritative cross-workspace naming references and the live `asma` Kontor realm.

## When to Load

Load this plan while implementing, reviewing, migrating, deploying, or verifying configuration-driven ESW/ECP/TSW/ASW/CSW and seat names. The approved naming decision remains the semantic authority; this plan records how the current implementation will be moved to it.

## 1. Closure goal

Kontor must publish and pin one immutable Team Definition JSON revision that owns native hierarchy and rendering. The topology specification remains a legality validator only. The daemon supplies durable structured values, renders exact configured bytes, refuses missing or ambiguous values before runtime mutation, and preserves exact native IDs during retitle.

The recommended ASMA revision renders:

```text
ESW • KBI-8049
├── ECP • KBI-8049
│   ├── LSA
│   └── TPM
├── TSW • KBI-8062
│   └── <registered delivery role code>
├── ASW • <scope item code> • <topic>
│   └── <configured advisor role code or exact distinct slot label>
└── CSW • <scope item code> • <topic>
    ├── SEAT A
    ├── SEAT B
    └── JUDGE
```

The exact separator is space + U+2022 BULLET + space (` • `). Container names carry scope and consultation topic. Seat names carry only the configured local role code or exact slot display label.

## 2. Current gap

The current implementation is deliberately explicit but assigns naming to the wrong specification:

- `ProjectSessionTopologySpec` owns `name_separator`, container templates and seat templates.
- operational topology revision 4 renders `AREA_CODE · ITEM_CODE`, including redundant scope on ECP and TSW seats.
- ASW/CSW containers use generic literals and have no persisted topic token.
- Committee ticket scope is dropped when its topology node is frozen.
- Advisor materialization launches only the first frozen seat.
- Committee seat rendering uses role code instead of configured `SEAT A`, `SEAT B`, `JUDGE` labels.
- native-name preview/apply can preserve IDs, but it derives desired names from the topology pin.
- observed topology projections do not currently expose the latest read-back native name.
- a connector alias accepted during ASMA-8062 bootstrap created duplicate noncanonical `jira` link evidence beside canonical `connector.jira`; alias handling must be canonicalized without deleting historical evidence.

## 3. Accepted implementation contract

### 3.1 Immutable Team Definition

Add a typed `TeamDefinitionSpec` catalog with an immutable `{id, version, canonical_hash}` identity. A definition contains:

- the topology-spec revision used only to validate legal kinds, projections and ancestry;
- the declared container hierarchy and per-kind projection/capability policy;
- one exact separator;
- configured prefixes and typed container templates;
- container-specific seat-template policy;
- exact configured slot display labels where role code alone is not unique or intended; and
- an explicit recommended-ASMA definition revision.

The project selects a default definition through preview/apply. Every epic freezes the selected exact definition. TeamRuns and consultations inherit the epic pin and cannot silently move to a newer revision.

### 3.2 Rendering tokens

The closed renderer gains only durable typed facts required by the approved templates:

- `PREFIX`
- `EPIC_ITEM_CODE`
- `TASK_ITEM_CODE`
- `SCOPE_ITEM_CODE`
- `TOPIC`
- `ROLE_CODE`
- `SLOT_DISPLAY_NAME`

The daemon, not callers or adapters, selects the value set for the semantic subject. Missing topic, item code, registered role code, label, hierarchy declaration or capability is a typed pre-mutation refusal.

### 3.3 Consultation identity and reuse

Invocation requires a bounded, normalized topic distinct from the full question. The topic, subject kind, optional task id and Team Definition pin are frozen before any native effect.

- same family + same epic/task subject + same normalized topic + same pinned profile/template/definition reuses the same consultation workspace and seats for follow-up rounds;
- a materially different subject or topic creates a distinct ASW/CSW;
- Advisor profiles may declare one or more seats and every frozen seat is materialized;
- Advisors remain independent and have no Judge or aggregate verdict;
- Committee cardinality and labels come from its pinned template/Team Definition mapping;
- ticket-scoped Committees retain their task id on the CSW topology node.

### 3.4 Migration and retitle

Add preview/apply operations that:

1. publish/select the new Team Definition under compare-and-swap;
2. preview an epic pin upgrade and every resulting native rename;
3. apply the pin and retitles only after the complete preview is still valid;
4. read back every exact native ID, parent, kind, cwd and title;
5. record changed, unchanged and `rename_pending` targets without fabricating success; and
6. replay idempotently without creating containers or seats.

Historical definitions, topology name fields, receipts and literal readbacks remain immutable evidence. New publications cannot make `ProjectSessionTopologySpec` a naming authority.

## 4. Requirements and evidence

| ID | Requirement | Required evidence |
| --- | --- | --- |
| REQ-001 | One canonical Team Definition validates, canonicalizes, publishes, lists and reads byte-for-byte. | Core/team/store/API/MCP/OpenAPI tests. |
| REQ-002 | Project selection and epic pinning are explicit, immutable and restart-safe. | Migration/store/loopback/export/restore tests. |
| REQ-003 | Every recommended container and seat name renders exact UTF-8 bytes from the pin. | Renderer and operational-fixture byte tests. |
| REQ-004 | Topology publications cannot become a second current naming authority. | Validation/refusal and compatibility tests. |
| REQ-005 | ASW/CSW persist subject/topic and reuse only the exact same consultation identity. | Schema/repository/loopback tests. |
| REQ-006 | Multi-seat Advisors and configured Committee labels materialize exactly once. | Runtime/application contract tests. |
| REQ-007 | Migration preview/apply preserves all native IDs and reports pending names honestly. | Fake and Paseo retitle tests plus live readback. |
| REQ-008 | API, MCP, generated CLI, console and docs expose the same pin and desired/observed names. | Contract snapshot, parity and frontend tests. |
| REQ-009 | Jira aliases canonicalize to `connector.jira` while old duplicate evidence remains readable and non-authoritative. | Store/application regression and migration test. |
| REQ-010 | Exact merged artifacts migrate the live realm and render `ESW • KBI-8049`, `ECP • KBI-8049`, and `TSW • KBI-8062`. | Backup, schema, hash, runtime ID and native-title receipts. |

## 5. Plan graph

```text
Wave 0 — contracts
  RED-01 Team Definition schema/validation/rendering
  RED-02 persistence, selection and epic-pin migration
  RED-03 consultation subject/topic/reuse and multi-seat behavior
  RED-04 identity-preserving upgrade/retitle

Wave 1 — domain and persistence
  RED-01 -> IMP-01 typed Team Definition and renderer
  RED-02 -> IMP-02 schema v77, repositories, export/restore
  RED-03 -> IMP-03 invocation/topic/reuse and seat materialization

Wave 2 — orchestration and public surfaces
  IMP-01 + IMP-02 -> IMP-04 project selection, epic pin and migration preview/apply
  IMP-01 + IMP-03 -> IMP-05 naming for materialization and retitle
  IMP-04 + IMP-05 -> IMP-06 API/MCP/CLI/OpenAPI/console parity

Wave 3 — release
  IMP-06 -> VER-01 focused tests and mutation ledger
  VER-01 -> VER-02 full local archive verification
  VER-02 -> REL-01 PR, merge, exact-master build/deploy
  REL-01 -> MIG-01 live backup, schema migration, definition publish/select/upgrade/retitle
  MIG-01 -> CLOSE-01 live readback, gates, Jira/Kontor closure and worktree cleanup
```

## 6. Owned implementation surfaces

- `crates/kontor-core/src/naming.rs`, topology/spec/repository IDs and projections.
- `crates/kontor-teams/src/spec.rs` and bundled Team Definition fixtures.
- `crates/kontor-store/migrations/0077_*.sql`, repository/export/import/backup code and schema tests.
- `crates/kontor-daemon/src/applications.rs` for selection, pins, rendering, consultation reuse and migration orchestration.
- `crates/kontor-api`, committed OpenAPI, generated CLI/MCP registry and console projections.
- `crates/kontor-runtime*` only where exact retitle or readback contracts need extension.
- repository README/architecture/configuration/recovery/change log and the authoritative parent naming references.

## 7. Mutation ledger

| Mutant | Killing test | Status |
| --- | --- | --- |
| Read current project default instead of the epic's frozen definition. | Existing-epic pin survives later selection test. | Pending |
| Normalize ` • ` to ` · ` or trim separator bytes. | Exact UTF-8 renderer test. | Pending |
| Let topology `name_template` override the Team Definition. | Conflicting legacy topology-name test. | Pending |
| Repeat item code/topic in a seat title. | Container-scoped seat matrix test. | Pending |
| Drop Committee `task_id`. | Ticket-scoped CSW item-code test. | Pending |
| Launch only the first Advisor seat. | Two-advisor materialization test. | Pending |
| Render Committee seats from role code. | Exact `SEAT A`/`SEAT B`/`JUDGE` test. | Pending |
| Reuse an ASW/CSW across different topics or subjects. | Consultation identity/reuse test. | Pending |
| Retitle a different native ID or accept absent readback. | Identity-preserving apply refusal test. | Pending |
| Treat `jira` and `connector.jira` as different active connectors. | Connector canonicalization test. | Pending |

The already-shipped TSW bootstrap checkpoint also has a killed branch-attestation mutant and passed 154 Paseo runtime contracts plus the complete archive gate.

## 8. Delivery and rollback

- GitHub Actions remain disabled by explicit Kontor policy; `python3 scripts/verify-tree.py --mode archive` is the merge gate.
- Before schema or native mutation, take a supported realm snapshot and inventory schema, cursor, merge SHA and binary hashes.
- Schema migration is forward-only. Rollback restores the pre-migration snapshot with the inventoried prior binaries after stopping the daemon through launchd.
- A native retitle failure leaves the new desired pin plus typed pending/drift evidence only if the apply contract explicitly permits partial runtime progress; it never edits Paseo state directly or changes a native ID.
- Existing sessions and historical receipts are never renamed in storage to make history appear current.

## 9. Completion rule

Complete only when all RED contracts are green, every recorded mutant is killed, the exact committed archive gate passes, the delivery PR is merged, exact master binaries are deployed, schema v77 is healthy, the recommended definition is published and pinned, live native IDs are preserved with exact new names read back, the high-stakes TeamRun artifacts/gates close, Jira `ASMA-8062` and the Kontor task are complete, and the implementation worktree is removed cleanly.
