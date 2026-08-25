VERDICT: PASS

# KON-OP-14 / ASMA-7941 — Release Notes

> **Date:** 2026-08-21 19:05 CEST
> **Status:** 🟢 Release gate — PASS
> **Author:** Architect · KON-OP-14 (replacement seat; the original architect seat
> died in the 2026-08-21 Codex outage)
> **Category:** report
> **Scope:** `asma-rs-kontor` PR [#47](https://github.com/Carasent-ASMA/asma-rs-kontor/pull/47)
> (`38efb87`) and PR [#48](https://github.com/Carasent-ASMA/asma-rs-kontor/pull/48)
> (`980ec8e`), both merged to `origin/master`
> **Summary:** Release evidence for preserving the *source system's* task
> lifecycle across an epic import, without letting an inherited terminal fact
> forge evidence of native Kontor closure. Migration `0042` is correctly ordered
> and reachable in the current v48 lineage, verified first-hand. One test-coverage
> gap and one operator trap are recorded as follow-ups; neither blocks the release.

---

## When to Load

**Load this document when:**

- integrating or auditing schema generation 42 (`imported_state`);
- changing epic-import replay semantics, `ensure_task`, or task-state updaters;
- diagnosing a `409 revision_conflict` from `epics:apply` / `epics:preview`;
- planning an operator upgrade or rollback across generation 42.

**Do NOT load for:** ASMA-7950 / per-epic execution scopes (schema 43), which
merged interleaved with this ticket and is a separate deliverable.

---

## Release identity

| Item | Revision |
| --- | --- |
| PR #47 (squash merge, OP-14 core) | `38efb8776b7c9b89331f842176141f4a69b1237e` |
| PR #48 (merge, OP-14 corrective + fmt) | `980ec8ecf23ca75a85eb3d1d4c7458a170154009` |
| Corrective commit | `7de2fae259d84628ffba6cc60cd4b0ccae10e4c5` |
| rustfmt repair carried on the branch | `b871b531f95fb2377baff009e48d83cfd5bd6a63` |
| Pre-existing evidence | [`MUTATION.md`](MUTATION.md) |
| Canonical scope | OP-REQ-030 / operational gap GAP-1 |

Both merge commits are ancestors of `origin/master`, so the ranges below resolve
regardless of which branch a shared checkout happens to be on.

### The commit range is not purely OP-14

`git log --oneline 38efb87~1..980ec8e` returns six commits, **two of which belong
to ASMA-7950**, because PR #49 merged into the same window:

```
980ec8e Merge pull request #48 … ASMA-7941      ← OP-14
7b4ab67 Merge pull request #49 … ASMA-7950      ← NOT OP-14
b871b53 chore(kontor-daemon): Format runtime validation test ASMA-7941
b85cbf5 fix(runtime): Scope Paseo execution per epic ASMA-7950   ← NOT OP-14
7de2fae fix(epics): Preserve native progress on import replay ASMA-7941
38efb87 feat(epics): Preserve imported task lifecycle ASMA-7941 (#47)
```

Topology: `38efb87` is a **squash** merge (single parent `ba82cec7`), which is why
`b0d58a6` — the commit MUTATION.md names — is not an ancestor of master; its
content landed squashed. `7de2fae` and `b85cbf5` both branch from `38efb87`
independently; `7b4ab67` merges the ASMA-7950 side, and `980ec8e` merges the
OP-14 side (`b871b53`) on top.

**OP-14's actual shipped surface is `38efb87` + `7de2fae` + `b871b53`.**
`b871b53`, despite its `ASMA-7941` subject, is a two-line `rustfmt` repair to
`crates/kontor-daemon/src/runtimes.rs`. It is not OP-14 logic: it fixes a master
**Format check** failure that OP-14 *inherited* — `ba82cec7` (PR #46, KON-OP-12)
already failed the same check, and the offending line came from `33f5c88`. Carrying
someone else's fmt repair on this branch is provenance noise, not a defect.

`38efb87` and `7de2fae` must be taken **together**. `38efb87` alone introduced a
replay regression (`if task.state != requested_state { conflict }`) that made an
identical manifest un-replayable the moment any task started work; `7de2fae`
removed it. Never cherry-pick `38efb87` on its own.

## What ships

- Migration `0042_imported_task_lifecycle.sql`: one nullable `tasks.imported_state`
  column, `CHECK (imported_state IS NULL OR imported_state IN ('ready','completed'))`.
  Additive `ALTER TABLE ADD COLUMN`; no backfill, no table rewrite.
- A deliberately narrow import vocabulary, `EpicImportStateDto = {ready, completed}`.
  `completed` is a **historical** fact from the source system and projects to
  `TaskState::Done` for dependency and backlog-count continuity.
- Provenance is cleared by the **first native lifecycle transition**. Both — and on
  current master, the *only* two — `UPDATE tasks` statements set
  `imported_state = NULL`: `policy.rs:1150` (guardrail park) and
  `repository.rs:7062` (`transition_task`).
- `native_done` (`kontor-daemon/src/applications.rs:1678`) refuses to treat an
  imported terminal fact as a closure certificate, so an imported `done` task
  synthesizes no gate receipts, no successful run, and no epic-closure evidence.
- Replay guards in `ensure_task` (`kontor-store/src/graph.rs:1797`, `:1809`):
  a contradictory re-declaration conflicts; a provenance-free task cannot be
  relabelled historical.
- Export/backup round-trip: `imported_state` is
  `#[serde(default, skip_serializing_if = "Option::is_none")]`, so a legacy
  backup parses and a NULL provenance is omitted. `EXPORT_SCHEMA_VERSION` is
  unchanged at 1.

### What OP-14 did *not* change

The brief's framing — "preserves task lifecycle rather than resetting it on
replay" — needs one correction. **Replay never reset task state.** Before OP-14,
`ensure_task` returned `Applied::Unchanged` for an existing task after a single
module check (`ba82cec7:crates/kontor-store/src/graph.rs:1441-1453`); the task row
was not touched. The lifecycle OP-14 preserves is the **source system's**, not
Kontor's. The replay-preservation property is nonetheless now nailed down by test:
`an_identical_manifest_reapplies_over_a_task_that_natively_progressed` asserts a
reapplied manifest leaves a task at `in_progress`.

## Compatibility and migration

**The API change is additive and non-breaking.** `import_state` is absent from
`EpicTaskRequest.required` in the OpenAPI contract (only `title` is required),
carries `#[serde(default)]` → `Ready`, and `EpicTaskRequest` does not use
`deny_unknown_fields`. Pre-OP-14 the API had **no** state field at all and the
daemon hardcoded `TaskState::Ready` (`ba82cec7:…/applications.rs:11333`), so an
existing caller that omits the field gets byte-identical behaviour.

**Idempotency receipts survive the upgrade.** This is the upgrade hazard that was
handled deliberately and is worth naming: `applications.rs:13529` omits
`import_state` from the recorded epic-apply intent whenever it is the `Ready`
default, "so an old apply receipt remains replayable after upgrade". Without it,
every pre-upgrade `apply_epic_graph` receipt replayed after upgrade would refuse
with *"idempotency key reused with a different dispatch payload"*
(`kontor-store/src/commands/intent.rs:104`).

**Downgrade is not supported**, consistent with the existing migration contract:
a pre-v42 binary opening a ≥42 database fails with `StoreError::DatabaseTooNew`
(`migrations.rs:290`, `:351`; covered at `tests/schema_v1.rs:1349`). It is a clean
refusal before any write, not a corruption — but it means rollback of the *binary*
past generation 42 requires restoring a pre-upgrade database file.

## Migration and lineage findings

Master is now at `SCHEMA_VERSION = 48`, six generations past OP-14. The lineage
question is therefore not "was 0042 right when it merged" but "is it right now".

1. **Ordering is compile-enforced.** `MIGRATIONS[41]` is
   `0042_imported_task_lifecycle.sql`, and
   `const _: () = assert!(MIGRATIONS.len() == SCHEMA_VERSION as usize)` makes the
   index↔version binding a build failure rather than a silent drift.
2. **`apply_pending` reaches it on every path.** The normal path is
   `for migration in &MIGRATIONS[pending..]` where `pending` is the database's
   current `user_version`, so a realm at 41 runs 0042–0048 in order and a realm
   at ≥42 correctly skips it. The historical operational-hardening lineage
   (`version 34|35` without `consultation_profile_revisions`) now iterates
   `MIGRATIONS.iter().enumerate().skip(33)` with a single skip-set entry.
   That matters: at OP-14's merge this branch was a **hand-enumerated list** that
   OP-14 had to append `MIGRATIONS[41]` to — precisely the fragility KON-OP-12
   had been bitten by one generation earlier. A later ticket replaced the list
   with the skip-set, so the hazard OP-14 participated in no longer exists.
3. **`0045` rebuilds the `tasks` table — and carries the column correctly.**
   This is the one place a silent data loss could have hidden, and **no test
   covers it**: the only lineage test that crosses v45
   (`the_merged_op12_v41_lineage_upgrades_through_epic_execution_scopes_v43`)
   seeds a task with **NULL** provenance and asserts NULL afterwards, which a
   rebuild that dropped the column from its `INSERT … SELECT` would also satisfy.
   I verified it first-hand instead (see below).
4. **No later ticket added an updater that forgets to clear provenance.** On
   current master there are exactly two `UPDATE tasks` statements and both set
   `imported_state = NULL`. The v45 `withdrawn` terminal state routes through
   `transition_task`, so it clears provenance like any other native transition.

### First-hand lineage verification

Replayed the real migration SQL with `sqlite3` against the files at `1370ba5`,
seeding **non-NULL** provenance before the v45 rebuild — the case no test covers:

| Step | Result |
| --- | --- |
| `.read` 0001 → 0044 in order | clean, `PRAGMA user_version` = 44 |
| seed 3 tasks: `completed`, `ready`, NULL provenance | written |
| apply `0045` (`DROP TABLE tasks` + rebuild) | clean, `user_version` = 45 |
| provenance after the rebuild | `completed` / `ready` / NULL — **all intact** |
| apply 0046 → 0048 | clean, `user_version` = **48** |
| provenance at v48 | `completed` / `ready` / NULL — **all intact** |
| 0042's CHECK still enforced at v48 | `INSERT … 'in_progress'` → *CHECK constraint failed* |
| `PRAGMA integrity_check; PRAGMA foreign_key_check` | `ok` |

`0045`'s implicit-column `INSERT INTO tasks_v45 SELECT …` is positionally exact
against the new table's declaration order, which is why this holds.

## Operator-facing release risk

The change is additive at the schema and API layers and fails closed everywhere I
could exercise it. The real risks are behavioural, not migrational.

1. **An imported `completed` task is born terminal and immutable.**
   `completed` projects to `Done`, and `tasks_terminal_immutable` (recreated by
   0045) permits only `done → ready`, while `tasks_no_delete` forbids deletion.
   A wrong `completed` in a manifest is therefore not a typo an operator can
   simply re-apply away: it requires an audited reopen with a command receipt.
2. **The reopen/progress trap — the one to brief operators on.** Any native
   transition clears provenance to NULL. A manifest that then declares that task
   `completed` hits *"an existing task without import provenance cannot be
   relabelled historical"* and conflicts **permanently**. The realistic trigger is
   not a mistake: a Jira-sourced manifest regenerated after Jira marks a task Done
   will declare `completed` for a task Kontor has already worked on. Because
   `apply_epic` is one transaction, **one such task refuses the entire epic
   apply.** Mitigation: nothing in Kontor derives `completed` from Jira today
   (`import_state` has no automatic producer — it is only ever hand-authored or
   agent-authored), so this fires only on a deliberately edited manifest.
3. **That refusal is opaque on the wire.** Every `RepositoryError::Conflict`
   maps to `409 revision_conflict` with the generic rule *"a persistence rule
   refused the write against the presented state"*; the specific rule is
   deliberately withheld from the caller and only `warn!`-logged
   (`kontor-api/src/error.rs:497-509`). An operator seeing `revision_conflict`
   will naturally re-read the aggregate and retry — which will fail forever.
   **The actionable rule is in the daemon log, not the response.** This is a
   pre-existing platform property, but OP-14 adds three new rules behind it.
4. **`epics:preview` is the safe probe.** `preview_epic` and `apply_epic` share
   `evaluate_epic`, differing only in `commit`, so a preview exercises the exact
   same conflict logic and rolls back. Preview every manifest that declares
   `completed` before applying it on a live realm.

### Rollback considerations

- **Forward-only.** Generation 42 cannot be un-applied by the binary; a pre-v42
  binary refuses a ≥42 database outright (`DatabaseTooNew`). Rollback past 42 means
  restoring a pre-upgrade database file, which discards everything since.
- **Rolling back the *code* but not the schema is safe.** `imported_state` is
  nullable and unread by pre-OP-14 code paths; a v48 database keeps the column
  regardless. The practical blocker is `SCHEMA_VERSION`, not the column.
- **Reverting the commits is not a rollback.** Reverting `38efb87`/`7de2fae`
  without also reverting `SCHEMA_VERSION` and the `MIGRATIONS` entry would trip
  the compile-time length assert; reverting all of it leaves any already-migrated
  realm unopenable. Treat generation 42 as permanent.
- **Backups are format-stable across this change.** `EXPORT_SCHEMA_VERSION`
  stays 1 and `imported_state` is an optional field, so backups taken either side
  of the upgrade interoperate.

## Verification

### The tree these tests actually ran against

The shared checkout `_tools/asma-rs-kontor` is worked concurrently by other
agents and is **not** on `master`. Recorded exactly:

| | |
| --- | --- |
| Shared checkout HEAD | `1a9354e09263cba3c58129afe51f64a7af9a87e4` (branch `feat/ASMA-7882-quota-signal-classifier`, one commit ahead of master) |
| Shared checkout `git status --porcelain` | `?? _docs/` and `?? docs/evidence/KON-MVP-18/run-55816edc06292067/` — **no tracked-file modifications** at the three points I sampled it |

The shared checkout moved **again** while this evaluation was running: by the time
the document was finalized it was on `fix/ASMA-7869-serve-api-during-startup-reconciliation`
at `4c6ebd5`. The `1a9354e` row above is a faithful record of the tree those tests
actually compiled, not of the checkout's current position — do not try to reproduce
them there. The clean worktree `~/.cache/op14-release/tree` @ `1370ba5` is the
reproducible one, and is left in place (registered but detached) alongside the
other agents' review worktrees; `git worktree remove` it when the epic closes.
| Clean worktree | `~/.cache/op14-release/tree` @ `1370ba5cbd84a93a6f09ca76eb57219472b07293` (`origin/master`), `git status --porcelain` empty, `SCHEMA_VERSION = 48` |

**These runs do not attest the merge commits themselves.** They attest current
master (`1370ba5`) and master-plus-one-unrelated-`kontor-accounts`-commit
(`1a9354e`). Both contain all of OP-14 and the full v48 lineage, which is the
release-relevant tree.

### Exit codes — real process codes, no pipelines

Logged to `~/.cache/op14-release/`.

**Clean worktree, `origin/master` `1370ba5`:**

| Command | Exit |
| --- | --- |
| `cargo test -p kontor-core` | **0** |
| `cargo test -p kontor-store` | **0** (328 passed, 0 failed) |
| `cargo test -p kontor-daemon` | **0** (231 passed, 0 failed) |
| `cargo test -p kontor-tests-contract` | **0** (104 passed, 0 failed) |

`kontor-store` is the package that owns the migration lineage, the park writer and
the export round-trip. On clean `origin/master` its OP-14-relevant tests pass by
name:

```
test the_merged_op12_v41_lineage_upgrades_through_epic_execution_scopes_v43 ... ok
test an_empty_database_migrates_to_the_current_schema_version ... ok
test a_schema_v1_database_is_upgraded_in_place_and_keeps_its_realm ... ok
test parking_an_imported_task_clears_its_historical_lifecycle_provenance ... ok
test imported_task_lifecycle_provenance_survives_export_serialization_and_parse ... ok
```

**Shared checkout, HEAD `1a9354e`:**

| Command | Exit |
| --- | --- |
| `cargo test -p kontor-core` | **0** |
| `cargo test -p kontor-api` | **0** |
| `cargo test -p kontor-mcp` | **0** |
| `cargo test -p kontor-daemon` | **0** |
| `cargo test -p kontor-tests-contract` | **0** |
| `cargo test -p kontor-store` | **143** — SIGTERM, **not a test failure** |
| `pnpm --filter kontor-console verify:api` | **0** (generated console types match the committed contract) |

The `143` is `128+15`: the run was killed mid-sweep by an unrelated session
interrupt. Its log contains **zero** `test result: FAILED` lines and zero panics.
It is reported here rather than quietly dropped, and re-run clean above.

All five OP-14 loopback tests pass by name on **both** trees (clean `1370ba5`:
231 passed / 0 failed in `kontor-daemon`):

```
test epic_import_preview_apply_and_replay_preserve_historical_task_lifecycle ... ok
test epic_import_defaults_ready_and_refuses_invalid_or_contradictory_state_atomically ... ok
test an_identical_manifest_reapplies_over_a_task_that_natively_progressed ... ok
test a_mixed_import_closes_after_only_its_native_task_earns_completion ... ok
test a_configured_jira_boundary_distinguishes_historical_from_native_completion ... ok
```

### #48's cancelled CI check — nothing is left unverified

PR #48's rollup shows one `CANCELLED` "Rust workspace gates" beside three
`SUCCESS`. Both runs have the **same `head_sha`**, `b871b531…` — PR #48's tip:

| Run | Event | Rust workspace gates | Console gates |
| --- | --- | --- | --- |
| `32303330030` | `push` | **SUCCESS** (21:42:06Z) | SUCCESS |
| `32303334866` | `pull_request` | CANCELLED (21:49:38Z) | SUCCESS |

A `push` and a `pull_request` trigger produced duplicate runs on one commit and a
concurrency group killed the redundant one. **The identical tree passed both gates
on the `push` run**, so the cancellation removes no coverage.

Two honest caveats found while establishing that:

- **The merge did not wait for the gate.** #48 merged at 21:29:40Z; the successful
  Rust gate finished at 21:42:06Z — 12 minutes *after* the merge. The tree is
  verified, but it was verified after landing, not before.
- **No master-branch run went green on any of the three merge commits.**
  `38efb87` **failed** (Format check only — inherited from `ba82cec7`, and the
  reason `b871b53` exists); `7b4ab67` and `980ec8e` were both **cancelled** by
  subsequent pushes to master. This is closed by descent, not by those runs:
  `054c3c5d` (2026-08-19T22:35:13Z), a **descendant of `980ec8e`**, went green
  ~1 hour after the merge, and master has been green since.

### Audit of MUTATION.md

Every claim I could check against the code held, with three corrections:

- ✅ Both provenance checks, the park's `imported_state = NULL`, the
  column-explicit inserters and the export column are all present as described,
  and all eight named killer tests still exist six generations later.
- ⚠️ **Line references have drifted.** `graph.rs:1487`/`:1499` are now `:1796`/`:1809`
  (later tickets); `policy.rs:1150` is still exact.
- ⚠️ **The inserter census undercounts.** MUTATION.md names "the two inserters
  (`graph.rs`, `intake.rs`)"; there are **three** — `repository.rs:770`
  (`create_task`) has existed since KON-MVP-03. Verified benign: column-explicit
  without `imported_state`, and its returned `Task` sets `imported_state: None`.
- ⚠️ **M4's receipt does not cover the branch it claims** — see the follow-up below.
- ℹ️ M1–M6 receipts are second-hand by the author's own disclosure ("builder
  handoff, not re-run here"). I did not re-run them; the mutation gate is not
  this gate. The disclosure is good practice and is why I audited the claims
  against the code rather than trusting the table.

## Follow-ups (not release blockers)

1. **`ensure_task`'s second guard is untested.** MUTATION.md's M4 row credits
   `epic_import_defaults_ready_and_refuses_invalid_or_contradictory_state_atomically`
   with killing "the contradiction check". That test's task was imported with
   `import_state` omitted, so it is stored as `Some(Ready)` and the re-declaration
   trips the **first** guard (`is_some_and(|s| s != plan)`). The **second** guard —
   `task.imported_state.is_none() && plan.imported_state != Ready`, which protects
   pre-v42 and natively-progressed tasks from being relabelled historical — has no
   test exercising its refusal path anywhere in the suite. A mutation deleting only
   that `if` would, on this reading, leave the suite green. Add a test that
   natively transitions an imported task and then re-declares it `completed`.
   *(Established by reading the cited test plus a repository-wide grep; I did not
   run the mutation.)*
2. **No test carries non-NULL provenance across the v45 `tasks` rebuild.** Covered
   first-hand above, but the suite should own it. Belongs to the ticket that owns
   generation 45.
3. **Consider surfacing an actionable code for import-declaration conflicts.**
   Collapsing them into `revision_conflict` invites an infinite retry loop
   (risk 3 above). A distinct non-enumerable code — or admitting these three
   rules to the caller, since a manifest author is not an untrusted client —
   would save an operator a log dive.

## Scope limits of this evaluation

- **I did not inspect the live realm database.** Two read-only attempts at
  `~/.local/state/kontor/asma/kontor.db` were blocked by the sandbox classifier
  and I did not work around it. Statements about the live realm's current
  generation and epic population rest on the LSA's brief, not on my observation.
  My lineage evidence is a faithful replay of the real migration SQL, not the
  production file.
- The tests attest `1370ba5` and `1a9354e`, not `980ec8e` (see above).
- ASMA-7950 / schema 43 shipped interleaved and is out of scope.

## Verdict

**VERDICT: PASS.** Migration `0042` is correctly ordered, compile-guarded, and
reachable on every `apply_pending` path in the current v48 lineage; I verified
first-hand that provenance — including non-NULL provenance, which no test covers —
survives the v45 `tasks` rebuild through to v48 with constraints and integrity
intact. The API and schema changes are additive and backward-compatible, the
pre-upgrade idempotency receipts were deliberately kept replayable, imported
terminality provably forges no native closure evidence, and every failure mode I
exercised fails closed with an atomic rollback. #48's cancelled check is a
duplicate-trigger cancellation on an identical SHA that passed, and the merged
tree went green on master by descent within the hour.

The gate is waivable; it is **not** being waived. This is a real pass, with three
recorded follow-ups — the untested second guard being the one worth scheduling —
and one operator briefing item: a regenerated manifest that declares `completed`
for a task Kontor has already touched will refuse the whole epic apply, and will
say only `revision_conflict` while doing it.

Per the epic's evidence convention this document supplies `release-notes`; the LSA
records the Kontor gate citing this verdict. I recorded no gate myself.
