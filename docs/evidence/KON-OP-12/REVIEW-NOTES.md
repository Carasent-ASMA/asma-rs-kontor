# KON-OP-12 code-review-gate — review notes

Verdict: **passed** (inspector seat, ASMA-7881).

Reviewed at the exact pushed head `9ca79f6d9b604ef639aae3d68d858cf6c9203268`
(code `a883f00`, evidence `9ca79f6`) on PR
[#45](https://github.com/Carasent-ASMA/asma-rs-kontor/pull/45).

Position verified independently: `merge-base(HEAD, origin/master)` ==
`origin/master` == `615d24bf658c8b00ab94a28664bffeca82b5d3fd`, so the branch is
exactly two commits ahead of current master with no rebase drift. Both required
GitHub checks are green at this SHA (Rust workspace gates, Console gates).

Judged against OP-REQ-038 as pinned by
[`architecture-handoff.json`](architecture-handoff.json) and its
`canonical_requirement`, the KON-OP-12 section of
`_docs/ai-orchestration/plans/2026-08-14-23-21-plan-kontor-operational-mvp.md`.

## Acceptance-risk questions carried into this review

Both were raised as explicit acceptance risks and were **not** treated as
automatically non-blocking. Both are dispositioned below against the pinned
scope, with exact evidence.

### AR-1 — the ledger has no production runtime caller / write surface

**Disposition: in scope, not a defect. Mandated by the handoff.**

The absence of an agent-reachable raise/dispose surface is required, not
omitted:

- `architecture-handoff.json` → `prohibitions[6]`: *"No public open-question
  API/CLI/MCP expansion unless the existing completion projection requires a
  typed blocker addition."*
- The requirement's **Owns** list covers the aggregate, migration, repository
  port, detector, policy predicate, role-instruction text and fixtures. No
  write surface appears in it.
- The requirement's **Implementation**: raising is *"stated as a duty in the
  resolved role instruction, **not implemented as a scanning service**"*, and
  closure routes *"through the existing surfaces only"*.
- Plan §5 wave table: `OP-08` owns CLI/MCP and performs the shared
  registry/generated updates serially; `OP-12` shares Wave 5 with it.

Held correctly and verified:

- `grep -rin "open_question\|OpenQuestion" crates/kontor-mcp/src crates/kontor-cli/src`
  returns **zero** hits.
- The only public-surface addition is the two additive `CompletionBlockerDto`
  variants in `crates/kontor-api/src/applications.rs:1165-1182`
  (`OpenQuestionUndispositioned`, `OpenQuestionReopened`) — exactly the carve-out
  `prohibitions[6]` permits. No existing field, route or DTO changed; the
  regenerated `openapi.json` (+50) and `schema.d.ts` (+14) are additive `oneOf`
  members.

The durable behaviour is reachable today through the narrow port —
`OpenQuestionRepository` (`crates/kontor-core/src/repository.rs:3179`),
implemented on `SqliteStore` (`crates/kontor-store/src/repository.rs:10047`).
Exposing it to agents is `OP-08`'s lane. Adding it here would have breached
`prohibitions[6]`.

### AR-2 — `CloseoutRecorded` has no end-to-end daemon construction path

**Disposition: pre-existing master-wide gap, not introduced or widened here,
and outside this ticket's Owns list. Not a defect of this PR.**

Exact evidence:

- On `origin/master` (`615d24b`), `CloseoutRecorded` occurs **only** at its
  definition (`crates/kontor-scheduler/src/completion.rs:515`), its match arm
  (`:737`), its name mapping (`:1027`) and in scheduler tests. There is **no**
  daemon construction site on master.
- At this head, the daemon's composed observation set is **byte-identical** to
  master: `TicketsClosed`, `VerdictRecorded`, `RemediationApproved`
  (`crates/kontor-daemon/src/applications.rs:6843`, `:6941`, `:11154`; master
  `:6826`, `:6924`, `:11123`). This PR removes no construction site and adds
  none.

So the missing closeout driver is inherited from OP-06's completion state
machine and belongs to the OP-06/OP-11 integration surface. The handoff scoped
OP-12 to *"Wire the **existing** `operational_default@1` completion path"* and to
*"Make only the **minimum** scheduler/daemon/API projection edits needed for
**this existing completion surface**"*. Building a new daemon closeout driver
would have exceeded that instruction.

The change additionally leaves the seam **stronger** than it found it.
`CloseoutRecorded` moved from a tuple variant to a struct variant carrying a
**mandatory** `open_questions: Vec<OpenQuestionSummary>`
(`crates/kontor-scheduler/src/completion.rs:536`). That is a compile-time
forcing function: whoever composes the closeout signal cannot record closeout
without supplying a question set, and an omitted set can no longer silently read
as "no open questions". The handoff's freshness rule (*"a fresh repository read
immediately before `MarkDone`"*, *"do not freeze the question set when completion
starts"*) is correctly located — `advance` is a pure state machine and cannot
read a repository, so the read obligation is pushed onto the composer and made
non-optional by the type.

Neither question makes the requested durable/dispositioned behaviour
operationally unreachable **within this ticket's pinned scope**, so neither is a
rejection ground. Both are carried forward as integration facts, not findings.

## Verified against the acceptance clauses

| Acceptance clause | Evidence |
| --- | --- |
| Cannot close without a disposition and an author | `dispose()` requires `author` + validated outcome, `open_question.rs:516`; `status()` returns `Open` with no disposition, `:443` |
| `deferred` refused without a concrete trigger | `ReopeningTrigger::validate` `:145`; SQL `CHECK ((kind = 'deferred') = (trigger_key IS NOT NULL))`, `0041:162` |
| A fired trigger reopens rather than leaving closed | `fire_trigger()` `:565`, exact-key match; `status()` → `Reopened` via `fired_against(current.ordinal, …)` `:608` |
| Superseded disposition stays readable, no in-place edit | Append-only `Vec`s; SQL `BEFORE UPDATE`/`BEFORE DELETE` `RAISE(ABORT)` triggers on all three child tables, `0041:119-128`, `:167-176`, `:229-238` |
| Epic with an undispositioned question cannot reach `done`, refusal names it | Gate at `scheduler/src/completion.rs:770-780`; test `an_undispositioned_question_keeps_the_epic_out_of_done` asserts phase stays `Closeout`, no `MarkDone`, and the blocker carries `question_id` + `subject` |
| Detector reports, mutates nothing | `detect(&DetectorObservations)` takes shared borrows only, no repository or command port, `:747`; findings are a plain `Vec` |
| Raise needs no capability; closing architectural from non-`LSA` refused | `raise()` takes any valid `SeatBindingId`; `dispose()` checks `CloserPolicy::closer_for(scope)`, `:520`, never a role literal |
| No second escalation path / notification / auditor role | No new aggregate, transport or role code in the diff; `prohibitions` held |
| Carries write-time `shareability`, defaults `project_shared` | Header stamp columns + `open_questions_shareability_is_attributable` trigger, `0041:40-43`, `:57-62` |
| Restart, export, restore preserve everything | Store suite: `every_round_disposition_and_firing_survives_a_restart`, `the_ledger_exports_deterministically_and_completely`, `the_ledger_survives_a_snapshot_restore`, `an_import_records_every_ledger_row_as_lineage` |

Also verified: tenant isolation is structural — child PKs/FKs are composite on
`project_id` (`0041:114`, `:161`, `:204`), not reliant on globally unique UUIDs;
`a_question_of_another_project_does_not_resolve` covers it. One-reopen-per-
deferral is enforced twice, by `UNIQUE (project_id, question_id,
disposition_ordinal)` (`0041:203`) and by `fire_trigger`'s `fired_against` guard.

Independent run of the acceptance-critical suite at this head:
`cargo test -p kontor-core --test open_question` → **27 passed, 0 failed**
(exit 0).

## The migration-lineage fix is real

`migrations.rs` converges the historical v35 lineage through an *explicit* list
rather than `MIGRATIONS[pending..]`. Adding `0041` to the inventory without
extending that list would have left any realm on that lineage converging to
version 40 and then failing to open. `MIGRATIONS[40]` is correctly appended
(`migrations.rs:354`). The pre-existing `[36]` skip in that list is untouched and
is not this ticket's concern. A fresh-database test cannot see this class of
defect; only the v35-lineage test can.

## Carried forward, not blocking

1. **Schema number collision.** This PR keeps `0041_open_questions`; `OP-08` also
   claimed 0041 and will integrate this merge first, then renumber. Already
   flagged in the PR body per LSA. The integrator must not merge these two blind.
2. **`CloserPolicy` has no production construction site yet.** The pack now
   carries `architecture_closer_code` / `process_closer_code`, validated against
   the pinned role catalog (`pack.rs:348-360`), so the data exists; binding it
   into a `CloserPolicy` lands with the write surface in `OP-08`. Same deferred-
   wiring class as AR-1.
3. **`CompletionState.open_questions` is a projection-only snapshot.** The gate is
   always judged on the signal's own fresh set; the stored copy exists so
   `blockers()` can name the question. Between closeout signals the *projection*
   can therefore be stale while the *decision* never is. Documented in the code
   and consistent with the handoff — noted so the tester does not read it as the
   gate input.

`import.rs` needing no edit is correct, not a gap: it is a selective **spec**
importer, and full restore is snapshot-based, which the handoff states follows
SQLite automatically. The four new tables are covered by the `exported_tables!`
macro, so export, `record_counts` and `lineage` pick them up automatically.
