# ASMA-8117 high-scope record

Date: 2026-09-06
Artifact: `high-scope-record`
Task: Jira `ASMA-8117` / Kontor `01a07722-c376-77f2-bee9-609793e172de`
Epic: Jira `ASMA-8049` / Kontor `01a0539a-51c9-7301-9bd7-26c09167b23e`
Project: `01a0064a-e056-7603-9968-ef64fdaacb75`
Phase: `high-scope`
TeamRun: `01a077ab-2555-7341-956e-5216a16ddb9a`

## Outcome

Add three new, typed Team Definition naming tokens whose values are exact
confirmed Jira keys:

- `EPIC_JIRA_KEY`
- `TASK_JIRA_KEY`
- `SCOPE_JIRA_KEY`

Use them to render epic, task and consultation containers without changing the
meaning of any existing token, any historical Team Definition revision, any
seat label, or any native identity. Prepare naming-only successor documents for
each live definition variant, but do not publish, select, deploy or migrate them
in ASMA-8117.

This scope is controlled by the approved plan
`asma-modules/plan/feature-kontor-jira-key-identity-1.md`, its governing
architecture amendment, Jira ASMA-8117 and the Kontor task above. AgentsRoom
backlog or memory and legacy mirror output are not authority for this record.

## Frozen baseline and bootstrap state

The canonical worktree is
`asma-modules/.worktrees/asma-8117/asma-rs-kontor` on branch
`feat/ASMA-8117-implement-jira-key-team-definition-tokens-and-rendering`.
The scope read was made at clean HEAD
`508a5141e0ecab8b207e1b7112f839e3f1bb9a2d`.

The deployed `origin/master` is two commits ahead:

- `c4a0e6bd462a9880f45982a8058907b931b810c2` — admit Jira-key-only task scopes;
- `177146015e186585d61e11c0ba1de7af92e4ee8c` — accept canonical catalog task
  worktrees, PR #205.

Before changing source, implementation must fast-forward this same branch and
worktree to `origin/master`. Do not create a replacement worktree or branch.

Kontor task revision 1 still projects `ready` in `high-scope`. That projection
does not mean no work is running: TeamRun
`01a077ab-2555-7341-956e-5216a16ddb9a` is `running`; its scope AgentRun
`01a077ab-2555-7341-956e-52211c01acff` and implementation AgentRun
`01a077ab-502e-7691-a139-4e3d71677700` are attached. The frozen verify AgentRun
`01a077ab-6ddc-7193-a59e-eed847eec6c5` is not attached.

## Scope boundaries

ASMA-8117 owns:

1. token declaration, parsing, serialization and value requirements in
   `crates/kontor-core/src/naming.rs`;
2. Team Definition template validation for the three new tokens while retaining
   the old token allow-list and meanings;
3. daemon container rendering from exact confirmed epic/task bindings in
   `crates/kontor-daemon/src/applications.rs`;
4. exact rendering, refusal, compatibility, subject-selection and successor
   fixture tests;
5. one killed subject-selection mutant.

ASMA-8117 does not own the shared public resolver and projections (ASMA-8116),
client integration and enforcement (ASMA-8119), deployment/publication/default
selection/live migration (ASMA-8120), or rollout closeout (ASMA-8121). It must
not change topology, allocate legacy backlog codes, transition Jira, publish a
Team Definition, rename a live container, or replace a TeamRun/AgentRun/seat.

## Required rendering contract

The literal separator stays space + U+2022 BULLET + space (` • `).

| Container | Template | Example |
|---|---|---|
| ESW | `PREFIX • EPIC_JIRA_KEY` | `ESW • ASMA-8049` |
| ECP | `PREFIX • EPIC_JIRA_KEY` | `ECP • ASMA-8049` |
| TSW | `PREFIX • TASK_JIRA_KEY` | `TSW • ASMA-8117` |
| ASW | `PREFIX • SCOPE_JIRA_KEY • TOPIC` | `ASW • ASMA-8117 • Naming review` |
| CSW | `PREFIX • SCOPE_JIRA_KEY • TOPIC` | `CSW • ASMA-8049 • Release readiness` |

`EPIC_JIRA_KEY` is always the confirmed key of the containing epic.
`TASK_JIRA_KEY` is always the confirmed key of the topology node's task.
`SCOPE_JIRA_KEY` is the confirmed task key when the subject is a task and the
confirmed epic key when the subject is the epic. Advisor and Committee names
follow the advised/debated subject recorded on their topology node, never the
caller or the containing epic by default.

The daemon's existing `container_name_from_definition` path is the single
rendering seam. Supply both old item-code values and the new Jira-key values to
`NativeNameValues`; do not fork a second renderer. A small private
`jira_key_for_subject` helper may centralize the exact confirmed-binding lookup
and be reused by `item_code_for_subject`. If the ASMA-8116 shared resolver is
already present after integration, use it instead of retaining duplicate lookup
logic.

A required new token with no confirmed binding must refuse before any native
prepare, retitle, launch or materialization effect. Foreign, malformed or
ambiguous bindings remain refusals owned by the confirmed-key contract. Do not
derive a key from a title, numeric suffix, legacy item code or UUID.

Seat rendering remains on its current `ROLE_CODE` / `SLOT_DISPLAY_NAME` path.
The exact configured local values, including `LSA`, `TPM`, `AUD`, `SEAT A`,
`SEAT B` and `JUDGE`, must remain unchanged and must not repeat a Jira key,
topic or container prefix.

## Historical compatibility

The existing tokens retain their exact serialized names and value semantics:
`EPIC_ITEM_CODE`, `TASK_ITEM_CODE`, `SCOPE_ITEM_CODE`, `AREA_CODE`, `JIRA_CODE`,
`KONTOR_BACKLOG_CODE`, `ITEM_CODE`, `AI_SHORT_NAME`, `PREFIX`, `TOPIC`,
`ROLE_CODE` and `SLOT_DISPLAY_NAME`. Existing pinned definitions continue to
render as before, and their canonical documents and hashes are never edited.

The legacy unpinned `container_name` fallback is outside this change and must
remain byte-for-byte compatible. Only immutable successor definitions may use
the new tokens.

## Live definition census and successor documents

A 2026-09-06 read of Kontor's project state found four exact definition
revisions pinned by epics with active topology:

| Source definition | Source canonical hash | Epic pins with active nodes | Prepared target |
|---|---|---:|---|
| Operational `01936f5a-2000-7000-8000-000000000001` v1 | `217747248d527556fa452a0b6380215a3699def6ba73cedbaabdc00da3b4da56` | 2 | same lineage v4 |
| Operational `01936f5a-2000-7000-8000-000000000001` v2 | `31cdff80e27cbe1e4043e150d2cdbc43ff79a8fa7fd8d892d2b5049a600f2e13` | 3 | same lineage v5 |
| Operational `01936f5a-2000-7000-8000-000000000001` v3 | `3818bade075891fe26fbc3d199b3f29204bc003082a1342983a208d3a868e35c` | 2 | same lineage v6 |
| Recovery `01a07400-1000-7000-8000-000000008098` v2 | `d2f1131e1b9548873c7a02a0c15761c982e70779c8d7e6ded872439659d79a43` | 1 | same lineage v3 |

Prepare four complete candidate documents plus a manifest under
`docs/evidence/ASMA-8117/team-definition-successors/`. The manifest must bind
each candidate to the exact source tuple and hash above. A candidate differs
from its source only in its immutable version and these container-template
token substitutions:

- ESW/ECP: `EPIC_ITEM_CODE` -> `EPIC_JIRA_KEY`;
- TSW: `TASK_ITEM_CODE` -> `TASK_JIRA_KEY`;
- ASW/CSW: `SCOPE_ITEM_CODE` -> `SCOPE_JIRA_KEY`.

Definition name, topology pin/hash, container order, kinds, prefixes, parentage,
capabilities, read-only policy, separator, role mappings, slot order, slot
cardinality and seat templates remain exact. The source documents and their
hashes remain untouched. Keep these candidates outside the bundled operational
profile so deployment cannot publish them implicitly; ASMA-8120 owns explicit
validate/publish/select receipts after the compatible fleet is deployed.

### OQ-8117-01 — successor addressing — RESOLVED

- **Subject:** immutable addresses for four naming-only successors.
- **Attaches to:** the successor manifest above and Team Definition lineages
  `01936f5a-2000-7000-8000-000000000001` and
  `01a07400-1000-7000-8000-000000008098`.
- **Why the state was ambiguous:** the approved requirement says each live
  variant receives a separate successor but does not assign target ids or
  versions. Three live variants already share the Operational lineage.
- **Options seen:** (a) allocate the next unused versions in the existing
  lineages and record the source mapping in the manifest; (b) create four new
  definition lineages; (c) prepare only the current project default and lose
  compatibility for epics pinned to v1/v2.
- **Disposition:** (a). `TeamDefinitionSpec` defines the stable lineage plus an
  immutable version; store publication is unique on the exact tuple and imposes
  no adjacent-version or predecessor relation; upgrade targets an exact
  published `{id, version}`. Existing lineages therefore preserve the intended
  identity, while sequential unused versions and the manifest make each source
  mapping explicit. Options (b) and (c) either invent lineage identities or
  violate the per-live-variant requirement.

No product or naming ambiguity remains open in this scope record.

## Verification contract

Implementation must leave these focused checks runnable:

1. Core naming tests render the exact byte strings in the table for all five
   container kinds, reject a missing required Jira token, preserve every old
   token spelling/meaning, and retain the current separator-glyph validation.
2. Team Definition contract tests deserialize and validate all four successor
   documents, prove their complete field equality to the recorded source except
   for version and the five allowed token substitutions, and assert the four
   historical source hashes.
3. Existing seat-rendering tests continue to assert the exact local labels and
   ignore leaked scope values.
4. Daemon loopback tests prove ESW/ECP use the confirmed epic key, TSW uses the
   confirmed task key, task-scoped ASW/CSW use the subject task key, epic-scoped
   ASW/CSW use the epic key, and a caller/containing-epic mismatch cannot change
   the subject.
5. The missing-confirmed-binding case refuses before every runtime mutation and
   keeps the old-token rendering case green.
6. Seed MUT-002 by changing task-scoped consultation selection to the
   caller/epic key. Record the exact changed line and command; the focused exact
   subject-rendering test must fail. Restore production code and rerun it green.

Suggested focused commands, to be adjusted only to the final test names:

```text
cargo test -p kontor-core --test team_definition_render_contract
cargo test -p kontor-core --test team_definition_naming
cargo test -p kontor-daemon --test loopback_api jira_key
```

These are required checks, not claims that the scope turn ran them.

## Operational gaps and owner handoff

### OP-8117-01 — frozen verify route is not currently bindable

The same scheduler start partially bound scope and implementation, then the
frozen high-stakes OpenCode verify route failed because live Paseo does not
advertise `providerOptionsApplied`. The aggregate returned `started=[]`, leaving
the task projection `ready` although its TeamRun is running.

Required runtime correction: the existing LSA/integration owner must restore
the required live Paseo capability advertisement and reconcile the same
TeamRun/verify AgentRun. Do not weaken the OpenCode posture proof, replace the
topology, create a substitute team, move the worktree, or widen ASMA-8117.
Implementation may proceed through the already-attached implementation seat;
verification cannot be claimed until the frozen verify seat binds and reports.

### OP-8117-02 — the Kontor LSA message path requires an unavailable refetch

The active ECP LSA seat is SeatBinding
`01a0539a-51e6-7d33-93d7-de8d116b7179`. A bounded Kontor handoff using canonical
message/idempotency UUIDv7 `01a077b8-40de-7db1-9fc6-1b4bc43f2b45` was refused
with `409 timeline_refetch_required`: the hosted session's content must be read
again from the runtime. No message-delivery receipt exists and no effect is
claimed. An earlier malformed non-UUID idempotency attempt was rejected before
delivery and is not a message identity.

Retry the same canonical UUIDv7 through
`kontor_topology_seat_message_send` after the hosted LSA canonical timeline can
be paged completely. Do not bypass Kontor, replace the LSA seat, or infer that
the runtime owner received this report.

## Normal handoff

Settle this `scope` turn with this artifact against task revision 1 and hand it
to the already-bound `implement` AgentRun
`01a077ab-502e-7691-a139-4e3d71677700`. The implementation seat must preserve
the canonical worktree, fast-forward the clean branch to deployed
`origin/master`, implement only the contract above, record its high-change and
mutation evidence, and then hand off to the original frozen verify slot when
the runtime capability gap is corrected.
