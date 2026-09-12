# ASMA-8117 implementation record

Date: 2026-09-06 (implemented), 2026-09-12 (rebased, re-verified, gate-1 and gate-2 repairs)
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

## High-verification gate 1 repairs

Gate 1 was REJECTED — receipt `01a09599-673a-7e72-aef2-cde9d8d799ed`, verifier
evidence hash
`a391c5e36f1925cf8f83d735be129ab74d6e7c2cb1e835856e7bd7c57bfc3837`. Four
findings, all repaired on this branch with `fcfbc714` preserved as history.

**1 — subject containment.** `subject_task_id` only referenced `tasks(id)`, so a
ticket from another project, or from a sibling epic in the same project, would
satisfy the foreign key while naming a subject the epic has no authority to
render. `tasks` is unique on `(project_id, id)` and SQLite cannot add a
composite foreign key through `ALTER TABLE ADD COLUMN`, so containment is now
enforced twice: `consultation_subject_is_contained` /
`consultation_subject_stays_contained` in storage, and an explicit check in
`create_consultation_run` that returns a legible
`Conflict { subject: "consultation subject" }`. Covered by
`a_subject_from_another_project_is_refused`,
`a_subject_from_a_sibling_epic_in_the_same_project_is_refused`,
`storage_refuses_an_uncontained_subject_even_without_the_repository_check`
(which drops the frozen-inputs trigger first, or it would pass without
containment being enforced at all), and the general relationship test
`the_run_node_and_subject_must_all_describe_one_scope`. Kill-proven: removing
both triggers makes the storage test fail on the cross-project subject.

**2 — unrelated evidence bundle.** `fcfbc714` swept 53 files of
`docs/evidence/KON-MVP-18/run-461f54595d89cec3` in through a `git add -A`; they
were also the only `git diff --check` violation. Removed in follow-up commit
`e4f0aff` rather than by rewriting history, leaving the rest of the KON-MVP-18
evidence untouched. `git diff --check origin/master..HEAD` is now clean.

**3 — Committee/CSW and a mutating refusal.** The Committee family now has its
own daemon test,
`committee_containers_follow_their_recorded_subject_not_their_caller`, split out
behind a shared fixture so a mutation cannot be caught only by the ASW rows —
both families now fail independently under MUT-002. The missing-binding refusal
no longer rests on a preview: the same withdrawn confirmation is driven through
`topology:materialize`, a mutating public route, asserting `placement_blocked`,
zero `PrepareContainer`/`Retitle*`/`Launch*` adapter calls, and that every bound
container title is byte-identical afterwards.

**4 — MUT-002 at the new head.** Re-seeded, killed and restored at the exact
current site; see below.

## High-verification gate 2 repairs

Gate 2 was REJECTED on evidence and hygiene only — receipt
`01a095e1-db95-73e3-8442-5a1beb0f536e`, verifier evidence hash
`d2520571ad425b11ef3f04991af29e5119172ed212ddf8b6feefe2d67c34fa54`. The
production repairs were independently accepted and are unchanged; nothing under
`crates/` was touched for gate 2.

**1 — a second generated bundle.** The gate-1 repair commit staged with
`git add -A` and swept in `docs/evidence/KON-MVP-18/run-08fbac17cca1d649`, a
directory the daemon suite generates as a side effect. Removed in follow-up
commit `1ba3c56`, staged by explicit path, after every test run for this round
had finished. Verified afterwards: no KON-MVP-18 run directory on this branch
is absent from master, and `git diff --check origin/master...HEAD` is clean.
`git add -A` is not used on this branch again.

**2 — MUT-002 covering both families in one command.** Recorded below with the
filter proof.

**3 — run identities.** The workspace record now names each run once with its
own log hash, and no sentence attributes two failures to the 2439-pass run. See
*Run identities* under Verification.

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

**Mutation site.** `crates/kontor-daemon/src/applications.rs:34466`, inside
`subject_task_for_container`, changing task-scoped consultation selection to
fall back to the caller/containing epic:

```text
-            Some(ConsultationSubject::Task(task_id)) => Ok(Some(task_id)),
+            Some(ConsultationSubject::Task(_)) => Ok(None),
```

**Unmutated source identity.** `crates/kontor-daemon/src/applications.rs`
SHA-256
`831e812c789cdac1d99670d2258849320205db95dcf93d923d727b1a2e24fca2`, captured
before seeding and again after restoring.

**The command.** One command, whose filter selects the two tests that assert
the ASW and CSW subject rows:

```text
cargo test -p kontor-daemon --test loopback_api -- \
    _containers_follow_their_recorded_subject_not_their_caller
```

Appending `--list` to that exact filter enumerates its selection and nothing
else — `committee_containers_follow_their_recorded_subject_not_their_caller`
and `consultation_containers_follow_their_recorded_subject_not_their_caller`,
count 2.

**Observed red, mutant in place.** Both families failed independently, each on
its own assertion rather than one riding on the other:

```text
test consultation_containers_follow_their_recorded_subject_not_their_caller ... FAILED
  left: "ASW • ASMA-463041493 • Naming review"
 right: "ASW • ASMA-518272654 • Naming review"
test committee_containers_follow_their_recorded_subject_not_their_caller ... FAILED
  left: "CSW • ASMA-73860482 • Naming review"
 right: "CSW • ASMA-518272654 • Naming review"

test result: FAILED. 0 passed; 2 failed; 0 ignored; 315 filtered out
```

i.e. the mutant rendered the containing epic instead of the advised ticket on
both ASW and CSW.

**Observed green, restored.** The file was restored from an untouched copy,
returning to the SHA-256 above with an empty `git diff`, and the same command
reran:

```text
test consultation_containers_follow_their_recorded_subject_not_their_caller ... ok
test committee_containers_follow_their_recorded_subject_not_their_caller ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 315 filtered out
```

### Workspace gates

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `cargo test --workspace --no-fail-fast` | run B below — 2439 passed, 1 failed, 9 ignored |

`--no-fail-fast` is used because a plain `cargo test --workspace` stops at the
first failing binary and would leave most of the workspace unreported. The nine
ignored are the suites' predeclared live-environment cases.

### Run identities

Three distinct runs are cited anywhere in this record. Each is named once, with
its own identity, and no observation is carried from one to another.

| Run | Command | Source | Log SHA-256 | Result |
| --- | --- | --- | --- | --- |
| **A** | `cargo test --workspace --no-fail-fast` | pre-repair tree, `fcfbc714` | `bea64c5ab1721b6769a6876997752cb4622924615319d992ce4f3852371d3792` | 2434 passed, **1** failed, 9 ignored, 123 binaries |
| **B** | `cargo test --workspace --no-fail-fast` | post-repair tree, later committed as `59a49fd` | `61a866646b3aea7855ee6b8e11ebdea68d47495c2ccb3da84b49edee8665b44b` | 2439 passed, **1** failed, 9 ignored, 123 binaries |
| **C** | `cargo test -p kontor-store --test consultation_subject --test schema_v1` | post-repair tree | not retained | one failure, named below |

Run **B** is the workspace record for this branch. It has **exactly one**
failing test. Run B is reused rather than repeated: every commit after
`59a49fd` changes only `docs/`, so no source file has moved since it ran, and
rerunning inside the worktree would regenerate the evidence bundle that was
just removed.

### The one failure in run B

`loopback_api::replaying_a_partial_admission_delivers_its_durable_follow_up`
is **pre-existing on master**. It fails deterministically (3/3 isolated runs)
with `revision_conflict: the task moved since the caller read it` against its
hard-coded `"expected_task_revision": 1`. The test body on this branch is
byte-identical to `origin/master`'s, and the same test fails identically on an
unmodified detached checkout of `e40b5f4` — same assertion, same line, same
refusal. This change writes no task revisions, so it is inherited breakage, not
a regression here. The throwaway probe worktree was removed; no branch, session
or external identity was created for it.

The same single test is also the only failure in run A.

### A separate observation, from run C only

`schema_v1::a_concurrent_first_open_initializes_exactly_one_realm` failed once
under **run C**, where a heavy suite runs alongside it. Four threads race a
first open through a barrier; it passed 5/5 in isolation and 57/57 twice with
`schema_v1` as its own suite.

This test did **not** fail in run A or run B, and it is deliberately not
counted against either. It is recorded here because this change does add a
migration, which lengthens first open — stated as observed behaviour under one
named command, not as proof of independence.

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
