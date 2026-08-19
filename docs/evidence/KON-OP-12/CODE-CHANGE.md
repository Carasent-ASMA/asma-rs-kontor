# KON-OP-12 / ASMA-7881 — code change

Date: 2026-08-19
Branch: `feat/ASMA-7881-kontor-ambiguity-disposition` (submodule `_tools/asma-rs-kontor`)
Baseline: created from `origin/master` `615d24b`, per the architect handoff's
`baseline.instruction`. The checked-out gitlink was `33e07a2` — 45 commits behind,
predating the merged OP-01/OP-05/OP-06 work and its schema-lineage convergence,
which is why the new migration is `0041` and not `0032`.
Requirement: OP-REQ-038 (open-question ledger).
Handoff: [`architecture-handoff.json`](architecture-handoff.json).

## What this delivers

An unresolved ambiguity becomes a durable, dispositioned record, and an epic
carrying an undispositioned one cannot reach `done`. Six pieces:

1. the `OpenQuestion` aggregate — immutable header, append-only history;
2. exactly three dispositions, with deferral reopening on its named trigger;
3. a report-only detector for the machine-checkable subset;
4. schema `v41`, a narrow repository port and its store implementation;
5. the completion gate, evaluated from a fresh read at the only `MarkDone`;
6. the open-question duty in every ordinary seat's launch instruction.

## Domain — `crates/kontor-core/src/open_question.rs` (new, 806 lines)

| Item | Line | Purpose |
| --- | --- | --- |
| `QuestionScope` | `:52` | `architecture \| product \| process \| routing`; recorded when raised, not derived from the attachment. |
| `QuestionScope::needs_architecture_closer` | `:78` | The two-way split: architecture/product are about *what is built*, process/routing about *how it is run*. |
| `DispositionKind` | `:83` | The persisted discriminator. Separate from the payload so an unknown spelling from SQL is refused, and a fourth way of closing cannot be added in one layer alone. |
| `DecisionCitation` | `:108` | Record **and** exact revision. A citation naming only the record would still look satisfied after that record was superseded. |
| `OpenQuestionAttachment` | `:118` | `Record(AggregateRef) \| Document(ContentHash)`. |
| `ReopeningTrigger` | `:132` | `key` is the identity a firing matches on; `condition` is its prose. Both mandatory. |
| `DispositionOutcome` | `:159` | `resolved(citation) \| deferred(trigger) \| not_relevant(reason)` — no `closed`, no `wontfix`. |
| `DispositionOutcome::validate` | `:194` | Refuses a deferral with no condition and a `not_relevant` with no reason. |
| `AmbiguityRound` | `:217` | Ordinal-addressed, immutable, optional `supersedes`. |
| `Disposition` | `:235` | Same shape; a correction appends and names its predecessor. |
| `TriggerFiring` | `:255` | Names the *disposition ordinal* it fired against, so reopening never depends on comparing two instants. |
| `OpenQuestionStatus` | `:268` | `open \| resolved \| deferred \| not_relevant \| reopened`, always derived — never stored. |
| `OpenQuestionStatus::blocks_completion` | `:294` | `open` and `reopened` block; all three dispositions release. |
| `CloserPolicy` | `:306` | The two closer role keys, as data. |
| `OpenQuestion::raise` | `:386` | Unprivileged: any valid seat. Stamps `Shareability::default_for(ProjectKnowledge)` → `project_shared`. |
| `OpenQuestion::status` | `:443` | Latest disposition wins; a deferral with a firing against it reads `reopened`. |
| `OpenQuestion::dispose` | `:516` | Authority checked against `CloserPolicy`, never a role literal. |
| `OpenQuestion::fire_trigger` | `:565` | Refuses a non-deferred question, a mismatched trigger and a second firing. |

### The detector — same file, `:673`–`:806`

`DetectorObservations` (`:691`) holds only shared borrows: no repository, no
mutable aggregate, no command port, so a detector that wanted to resolve a
question has nothing to reach for. `detect` (`:747`) returns
`Vec<OpenQuestionFinding>` (`:707`) covering the three findings the requirement
names — contradicting accepted decisions, a superseded citation, a fired
deferral trigger. Order is a deterministic pass order (subject order, then
question-id order) rather than a sort, because `AggregateRef` is not `Ord`.

## Ports — `crates/kontor-core/src/repository.rs:3179`

`OpenQuestionRepository`: `raise_question`, `get_question`,
`list_questions_for_epic`, `summarize_questions_for_epic`,
`append_question_round`, `append_question_disposition`,
`fire_deferred_trigger`. No generic update, no delete. One operation records both
a first closing and a correction — a supersede *is* an appended disposition that
names the one it replaces, and two operations would imply the second could edit
the first.

## Schema — `crates/kontor-store/migrations/0041_open_questions.sql`

One head row (`open_questions`) plus three append-only children. Three domain
rules are enforced in SQL, not only in the aggregate, because a rule that lives
in one layer is a rule direct SQL walks around:

- a `deferred` disposition must name its trigger, and only a deferral may
  (`CHECK ((kind = 'deferred') = (trigger_key IS NOT NULL))`);
- a firing must match the exact trigger its deferral named (trigger
  `open_question_firing_matches_its_deferral`);
- one deferral reopens at most once (`UNIQUE (project_id, question_id,
  disposition_ordinal)`).

`open_questions_only_the_revision_moves` refuses any header change but the
revision. All three child tables refuse `UPDATE` and `DELETE` outright. Every
child key carries `project_id` and every foreign key into the head is composite,
so tenant isolation does not rest on UUIDs being globally unique.

`SCHEMA_VERSION` moved 40 → 41 (`crates/kontor-store/src/migrations.rs:34`).

### One change beyond the handoff's file map — and it is a real defect, not a pin

`migrations.rs` carries a convergence path for the historical lineage where
master and the operational-recovery branch both shipped schema v35 with
different objects. That path does **not** run `MIGRATIONS[pending..]`; it runs an
explicit list, which ended at `MIGRATIONS[39]` (`0040_advisor_advice`). Appending
`0041` to the inventory therefore left any realm on that lineage converging to
version 40 and failing to open — `StoreError::Pragma { pragma: "user_version" }`.

Fixed by adding `MIGRATIONS[40]` to that list. This is exactly the
"schema-lineage convergence" the handoff's baseline note warned the stale gitlink
predated, and it is why `the_operational_hardening_v35_lineage_converges_without_losing_its_receipt`
exists: the defect is invisible to a fresh-database test and only a
historical-shape test catches it.

## Store — `crates/kontor-store/src/repository.rs:10047`

`impl OpenQuestionRepository for SqliteStore`. Every mutating operation appends
its child row and advances the head under compare-and-swap in one transaction,
so a caller working from a stale revision writes neither.
`summarize_questions_for_epic` derives status from loaded aggregates rather than
from a status column — a stored status could disagree with the history that
produced it, and this is the read the completion gate trusts.

## Backup — `crates/kontor-store/src/backup/export.rs`

Four typed row declarations added to `exported_tables!`. Open questions are
project *state*, not versioned specifications, so — like `tasks`, `context_packs`
and `handoffs` — they export with typed deterministic rows and are recorded in
import lineage rather than gaining a materialize path. Snapshot restore is a
byte-level database copy and follows automatically; both are asserted.

## Completion gate

| Layer | Line | Change |
| --- | --- | --- |
| `kontor-policy/src/completion.rs` | `:283` | `OpenQuestionBlocker` — `Undispositioned` / `Reopened`, each carrying id **and** subject. |
| `kontor-policy/src/completion.rs` | `:323` | `open_question_blockers` — pure, stable question-id order. |
| `kontor-scheduler/src/completion.rs` | `:536` | `CloseoutRecorded` became a struct variant carrying `open_questions`. The set is **required**, not optional: an omitted set would read as "no open questions" and let an epic finish over an unresolved ambiguity. |
| `kontor-scheduler/src/completion.rs` | `:777` | `MarkDone` requires both gates empty. A blocking question keeps the phase in `closeout` — the epic is not wrong, it is not finished. |
| `kontor-scheduler/src/completion.rs` | `:845` | `CompletionBlocker::OpenQuestion`, projected by `blockers` and `outstanding`. |
| `kontor-scheduler/src/completion.rs` | `:453` | `CompletionState.open_questions`, `#[serde(default)]`, stored **only** for the projection. |
| `kontor-api` / `kontor-daemon` | — | Two `CompletionBlockerDto` variants and their mapping. This is the one addition the prohibition list permits, and no route, MCP tool or CLI command was added. |

`advance` is pure, so freshness is enforced by the type: the observation cannot
be constructed without a question set, and the daemon reads the ledger to build
it. There is exactly one `MarkDone` emission and one `phase = Done` assignment in
the workspace, both inside the gated branch — verified by grep, so no path
reaches `done` around the gate.

## Role instruction — `crates/kontor-daemon/src/applications.rs`

`slot_prompt` (`:5662`) now appends `OPEN_QUESTION_DUTY` (`:5688`) to **both**
branches. The downstream seat's wait instruction is unchanged and still leads;
the duty follows it, because the ambiguity a waiting seat must record is
frequently in what it was just handed. The text creates a duty and nothing else —
no scanner, no capability, no role, no standing run.

## Closer codes — `crates/kontor-profiles`

`OperationalDelivery` gained `architecture_closer_code` (`pack.rs:266`) and
`process_closer_code` (`:268`), seeded as `LSA` and `TPM` in
`fixtures/operational-domain.json` and validated against the pinned role catalog
alongside `control_role_code` (`:353`). `kontor-core` is handed the two codes and
never learns what they spell.

## Prohibitions

| Prohibition | Held because |
| --- | --- |
| No Escalation aggregate / second state machine | None added; deliberation reuses Advisor/Committee and `NEEDS_HUMAN`. |
| No notification transport or prompt channel | None added. |
| No ambiguity-auditor role, seat, scan or service | Raising is an instruction; the detector is a pure function nobody schedules. |
| No detector-side mutation | `DetectorObservations` holds only shared borrows; asserted byte-identical across `detect`. |
| No raw role-name authorization in generic core | `CloserPolicy` is data; a renamed-closer test proves the old spelling loses standing. |
| No in-place `UPDATE`/`DELETE` of history | Schema triggers; asserted against raw SQL. |
| No public API/CLI/MCP expansion | Only the two permitted `CompletionBlockerDto` variants. |
