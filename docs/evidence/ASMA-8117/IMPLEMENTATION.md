# ASMA-8117 implementation record

Date: 2026-09-06 (implemented), 2026-09-12 (rebased and re-verified)
Artifact: `high-change`
Task: Jira `ASMA-8117` / Kontor `01a07722-c376-77f2-bee9-609793e172de`
Epic: Jira `ASMA-8049` / Kontor `01a0539a-51c9-7301-9bd7-26c09167b23e`
Phase: `implement`
AgentRun: `01a077ab-502e-7691-a139-4e3d71677700`

Controlled by `HIGH-SCOPE-RECORD.md` beside this file, its approved plan and
the Kontor task above. This record is implementation evidence only: it claims
no verification seat, no deployment, no publication and no Jira transition.

## Bootstrap

The canonical worktree `asma-modules/.worktrees/asma-8117/asma-rs-kontor` and
its branch were preserved throughout. No worktree, branch, session, topology
node, TeamRun, AgentRun or external identity was created or replaced.

Integration history, all on the same branch:

1. Fast-forwarded from the frozen scope base
   `508a5141e0ecab8b207e1b7112f839e3f1bb9a2d` to
   `177146015e186585d61e11c0ba1de7af92e4ee8c` (0 ahead, 2 behind, `--ff-only`).
2. Rebased onto `40ad3a54bce07b78d29270456f64b3ed72083f5c` (ASMA-8111, PR #202),
   which published `0093_consultation_session_releases.sql`; this change was
   renumbered 0093 -> 0094.
3. Rebased onto `e40b5f42759c06c15ea8d666a18b843d950db51d` (26 commits, through
   ASMA-8154 / PR #214), which published its own
   `0094_jira_description_projection.sql` and took `SCHEMA_VERSION` to 94; this
   change was renumbered 0094 -> **0095**, `PRAGMA user_version = 95`,
   `SCHEMA_VERSION = 95`, with a v95 sentence added to the `schema_v1`
   narrative.

The four conflicts in step 3 were all resolved additively rather than by
preferring a side: the migration registry (both migrations kept), the
`schema_v1` narrative (both sentences kept), `backlog_identity.rs` (master's
`LegacyEpicBacklogCode` and this change's `ConfirmedJiraKey` both kept, with a
duplicated doc anchor dropped), and `loopback_api.rs` (both appended test
blocks kept, each with its own closing).

Nothing on master has touched `consultation_run_inputs_are_frozen` since v92,
so the v92 body remains the correct base for the recreation below.

## What was implemented

### 1. Typed tokens and their values (`crates/kontor-core`)

- `naming.rs` declares `EPIC_JIRA_KEY`, `TASK_JIRA_KEY` and `SCOPE_JIRA_KEY` in
  the closed `NativeNameToken` vocabulary, with `with_epic_jira_key`,
  `with_task_jira_key` and `with_scope_jira_key` builders and a named
  missing-value refusal for each. The builders take a parsed
  `ConfirmedJiraKey`, so a caller cannot pass a string it assembled itself.
- `backlog_identity.rs` adds `ConfirmedJiraKey`, which admits only the
  canonical `<PROJECT>-<positive decimal>` spelling. The existing
  `JiraItemCode::derive` now delegates to it, so one admission serves both the
  historical item-code projection and the new tokens.
- `spec.rs` turns the Team Definition template rule from a deny-list into an
  explicit allow-list that admits the three new tokens. A token added to the
  vocabulary later is refused here until this contract deliberately admits it.

Every historical token keeps its exact spelling and meaning. The separator
remains ` • ` (SPACE, U+2022, SPACE) and its glyph validation is unchanged.

### 2. Durable consultation subject (`kontor-store`, `kontor-core`)

Rendering `SCOPE_JIRA_KEY` for a task-scoped ASW/CSW required durable subject
state that did not exist. The invocation's `task_id` was used for
authorization and folded into the semantic identity hash, then discarded:
`create_consultation_run` wrote `topology_nodes.task_id` as a literal `NULL`,
and `StoredConsultationRun` had no subject field.

`topology_nodes.task_id` cannot carry it. `ux_topology_node_task`
(migration 0027) enforces one active node per task so that "the task's
workspace" is a single answer at admission; a consultation about a ticket
would collide with that ticket's own TSW. That invariant is preserved
unchanged, and a first attempt to reuse the column was reverted when it
violated it.

Migration `0095_consultation_subject.sql` records the subject beside the run:

- `subject_kind` — `'epic'`, `'task'`, or `NULL`;
- `subject_task_id` — the exact advised ticket when the kind is `'task'`;
- a null-safe exhaustive column `CHECK` refusing every other pairing. It is
  written with `IS` rather than `=`, because SQLite counts a `CHECK` that
  evaluates to NULL as a pass: an `=` form admits `(NULL, <task>)`, a ticket
  recorded against a run whose subject is supposedly unrecorded. Reverting the
  constraint to that form makes the schema test below fail on exactly that
  pairing;
- the `consultation_run_inputs_are_frozen` trigger recreated from the *exact*
  schema-v92 body with only two null-safe predicates appended
  (`OLD.subject_kind IS NOT NEW.subject_kind`,
  `OLD.subject_task_id IS NOT NEW.subject_task_id`), so the subject is frozen
  semantic input and cannot be written onto a historical run after the fact.

  SQLite has no "alter trigger", so adding a predicate means recreating the
  whole body. An earlier draft recreated it from the older 0036 body, which
  silently withdrew v92's settled-topic-correction exception and regressed the
  deployed ASMA-8114/PR200 recovery path. That was caught in review and
  reproduced: with the pre-v92 body restored,
  `a_legacy_topic_correction_preserves_the_run_and_replays_one_authority`
  fails. The v92 body is now carried byte-for-byte and asserted as such.

`ConsultationSubject` (`kontor_core::consultation`) is the typed value;
`from_stored` refuses an impossible pairing rather than guessing. `NULL` is
deliberately not backfilled: the discarded value survives nowhere, and
asserting `'epic'` for historical runs would record something never observed.

### 3. Rendering (`crates/kontor-daemon/src/applications.rs`)

- `jira_key_for_subject` is the single confirmed-binding lookup. A missing,
  ambiguous or non-canonical binding refuses with `placement_blocked` before
  any native prepare, retitle, launch or materialization.
  `item_code_for_subject` now reuses it, so both vocabularies resolve one
  binding one way.
- `subject_task_for_container` reconciles the two durable shapes: a delivery
  workspace carries its task on the node; a consultation carries its frozen
  subject on the run. A consultation whose subject was never recorded refuses.
  Neither the caller's seat nor the containing epic is consulted.
- `container_name_from_definition` remains the single rendering seam. Both the
  old item-code values and the new Jira-key values are supplied to the one
  `NativeNameValues`; each is resolved only when the pinned template asks for
  it, so a Jira-key-only epic never has to produce a legacy item code.

Seat rendering is untouched and stays on `ROLE_CODE` / `SLOT_DISPLAY_NAME`.
The legacy unpinned `container_name` fallback is untouched. No pinned
definition, canonical document or hash was edited.

### 4. Successor documents

`team-definition-successors/` holds four complete candidates, their four exact
source documents and a manifest binding each pair. Sources were read through
the supported `kontor team-definition-get` route; all four canonical hashes
match the census in the scope record exactly, and the contract test re-derives
them through the production canonicalizer. The candidates are deliberately
outside the bundled operational profile. Nothing here validates, publishes,
selects, deploys or migrates them — ASMA-8120 owns those receipts.

OQ-8117-01 is resolved as recorded: option (a), the next unused version in each
source lineage (Operational v1/v2/v3 → v4/v5/v6, Recovery v2 → v3).

## Verification

Focused commands, adjusted to the final test names:

```text
cargo test -p kontor-core --test native_naming
cargo test -p kontor-core --test team_definition_render_contract
cargo test -p kontor-core --test team_definition_naming
cargo test -p kontor-core --test team_definition_successors
cargo test -p kontor-store --test consultation_subject
cargo test -p kontor-store --test schema_v1
cargo test -p kontor-store --test team_definition_persistence
cargo test -p kontor-daemon --test loopback_api -- jira_key_containers \
    consultation_containers_follow a_consultation_with_no_recorded_subject
```

All results below were re-run on the final rebased source
(`origin/master` `e40b5f4`), not carried over from an earlier base.

Against the contract's six required checks:

| Required check | Where | Result |
| --- | --- | --- |
| 1 — exact bytes for all five kinds, missing-token refusal, old spellings, separator | `native_naming`, `team_definition_render_contract` | PASS |
| 2 — four successors deserialize, validate, differ only by version and the agreed tokens, four source hashes | `team_definition_successors` | PASS |
| 3 — seat labels exact, leaked scope ignored | `team_definition_render_contract` | PASS |
| 4 — ESW/ECP epic key, TSW task key, task- and epic-scoped consultations, caller mismatch | `loopback_api` | PASS |
| 5 — missing confirmed binding refuses before every mutation; old-token case green | `loopback_api`, `team_definition_render_contract` | PASS |
| 6 — MUT-002 | below | KILLED |

The migration's own guards: `schema_v1` pins `SCHEMA_VERSION` at 95 with its
narrative; `the_database_refuses_every_impossible_subject_pairing` proves the
column constraint rejects all six impossible pairings and admits exactly the
three legal ones; and `the_settled_topic_correction_may_not_also_move_the_subject`
proves a v92 topic correction still moves a topic on v95 while a changed
subject on the same statement is refused.

The schema constraint carries its own kill proof: with the constraint restored
to a `=` form, `the_database_refuses_every_impossible_subject_pairing` fails on
`subject_kind=None with subject_task_id=Some(..)`. The corrected constraint was
then restored and the store suite reran green.

Subject durability, as required through create, reconciliation, retry and
restart: `consultation_subject` proves write/read round-trip for both subject
kinds, the preserved one-delivery-workspace invariant, the frozen-subject
refusal, and survival of every subject state — `Task`, `Epic` and historical
absent — across both a restart and a backup/restore, each asserted against the
original run and topology-node identities. The redacted hand-out export
carries no consultation subject; that is supplemental boundary evidence, not
the recovery proof. Retry is proven at the daemon level: a replayed invocation
under the same idempotency key renders the subject it froze the first time.

### MUT-002

Seeded in `crates/kontor-daemon/src/applications.rs:33400`, changing
task-scoped consultation selection to fall back to the caller/containing epic:

```text
-            Some(ConsultationSubject::Task(task_id)) => Ok(Some(task_id)),
+            Some(ConsultationSubject::Task(_)) => Ok(None),
```

```text
cargo test -p kontor-daemon --test loopback_api -- \
    consultation_containers_follow_their_recorded_subject_not_their_caller
```

FAILED as required — `left: "ASW • ASMA-76098410 • Naming review"` against
`right: "ASW • ASMA-518272654 • Naming review"`, i.e. the mutant rendered the
containing epic instead of the advised ticket. Production code was restored
from an untouched copy and the same command reran green.

### Workspace gates

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `cargo test --workspace --no-fail-fast` | 2434 passed, 1 failed, 9 ignored, 123 binaries |

The nine ignored are the suites' predeclared live-environment cases. The one
failure is analysed below; `--no-fail-fast` was used because a plain
`cargo test --workspace` stops at the first failing binary and would have left
most of the workspace unreported.

Two failures appear in the workspace run. Neither is introduced by this change,
and neither is dismissed as noise — both were attributed before being reported.

**`loopback_api::replaying_a_partial_admission_delivers_its_durable_follow_up`
— pre-existing on master.** It fails deterministically (3/3 isolated runs) with
`revision_conflict: the task moved since the caller read it` against its
hard-coded `"expected_task_revision": 1`. The test body in this branch is
byte-identical to `origin/master`'s, and the same test fails identically on an
unmodified detached checkout of `e40b5f4` — same assertion, same line, same
refusal. It is therefore master breakage that this branch inherits, not a
regression here. This change does not write task revisions. The throwaway
probe worktree was removed; no branch, session or external identity was
created for it.

**`schema_v1::a_concurrent_first_open_initializes_exactly_one_realm` — load
contention, matching the flake ASMA-8116 recorded.** Four threads race a first
open through a barrier. It passed 5/5 in isolation and 57/57 twice when
`schema_v1` runs as its own suite; it fails only when a heavy suite runs
alongside it. This change does add a migration, which lengthens first open, so
the attribution is stated as observed behaviour rather than as proof of
independence.

## Open question

### OQ-8117-02 — hyphenated project keys in the confirmed-key contract

- **Subject:** whether a confirmed Jira key may contain a hyphen inside its
  project key.
- **Attaches to:** `ConfirmedJiraKey` / `JiraItemCode::derive` in
  `crates/kontor-core/src/backlog_identity.rs`.
- **Why the state is ambiguous:** the ASMA-8050 validator admits `-` at any
  index after the first, so `KOP-8117-1` splits into project `KOP-8117` and
  suffix `1`. Real Jira project keys are `[A-Z][A-Z0-9]*`. The original commit
  records no rationale for the allowance.
- **Options seen:** (a) render the historical contract's exact output and
  leave the rule to the scope that owns the resolver; (b) tighten the project
  key here.
- **Disposition:** (a). ASMA-8117 was told these refusals *remain* owned by the
  confirmed-key contract, and tightening would newly refuse any live epic
  already bound to such a key — a live-compatibility change outside a
  naming-only scope. The behaviour is pinned by an explicit test so it cannot
  drift unnoticed, and ASMA-8116/8119 can decide it deliberately.

No other product or naming ambiguity is open in this scope.

## Boundaries observed

No topology change, no legacy backlog code, no Jira transition, no Team
Definition published or selected, no live container renamed, no
TeamRun/AgentRun/seat replaced, and no change to the deployed fleet. The four
source documents were read read-only through `kontor team-definition-get`;
that route requires admin authority, so the read was made with the realm's
admin secret and mutated nothing.

## Operational checkpoint

### OP-8117-03 — the durable implement→verify handoff is blocked

The implementation seat is attached and healthy: AgentRun
`01a077ab-502e-7691-a139-4e3d71677700`, role `implement`, TeamRun
`01a077ab-2555-7341-956e-5216a16ddb9a`, binding
`01a077ab-502e-7691-a139-4e49d7e07ad6` generation 1 on `paseo-local` native
`7fc3e7fe-64bf-4c4a-a7a0-06cdc1633d8a`, revision 12, projection `stale`. The
governing template `01936f5a-0000-7000-8000-000000000102` v1 declares slots
`scope, implement, verify, audit`, so the handoff target is `verify`.

The settle was attempted and refused:

```text
kontor turn-settle --project-id 01a0064a-e056-7603-9968-ef64fdaacb75 \
  --agent-run-id 01a077ab-502e-7691-a139-4e3d71677700 --role-slot implement \
  --expected-task-revision 2 --artifacts '["high-change"]' \
  --idempotency-key asma-8117-implement-to-verify-20260912-1

409 revision_conflict
rule: settlement requires the exact current runtime message and terminal
      timeline position
```

`TurnRuntimeProofRequest.message_id` must be the Kontor message identity the
runtime echoed on the current user turn. There is none to echo: all six items
in the current epoch (anchor
`01a077ab-502e-7691-a139-4e49d7e07ad6:4:6`) carry `message_id: null`, because
this turn was resumed through the approved bounded fallback rather than
`kontor_topology_seat_message_send`, which itself refuses `stale_binding`
against this daemon's missing frozen capability snapshot.

No proof was fabricated. A synthesized message id or timeline position would be
falsified control-plane evidence, which is worse than an unsettled turn. The
durable artifact for this turn is therefore this record and its commit; the
control-plane settlement remains Kontor's and is blocked on restoring a
dispatched-message path to this seat.

### OP-8117-01 — verify route, updated

The frozen OpenCode verify predecessor AgentRun
`01a077ab-6ddc-7193-a59e-eed847eec6c5` was abandoned at revision 4 under
receipt `01a077bf-1ef9-7f93-8a5a-62e58e3d531f`. The approved successor route is
`codex-personal / gpt-5.6-sol / max`, and it remains fenced until the durable
implementation→verify handoff this record is part of.

This seat does not create that successor and does not claim verification. The
implementation seat's obligation ends at a durable handoff; the successor is
bound and reported by its own route under the owning scope.

### OP-8117-02 — the Kontor LSA message path

Unchanged from the scope record. Canonical message/idempotency UUIDv7
`01a077b8-40de-7db1-9fc6-1b4bc43f2b45` was refused with
`409 timeline_refetch_required`, no delivery receipt exists, and no effect is
claimed. It must be retried through `kontor_topology_seat_message_send` once
the hosted LSA canonical timeline can be paged completely.

Nothing here bypasses Kontor, replaces the LSA seat, or infers that the
runtime owner received any report.

Verification cannot be claimed in this record; it hands off to the fenced
successor verify slot above.
