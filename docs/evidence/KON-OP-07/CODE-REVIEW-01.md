# KON-OP-07 / ASMA-7876 — inspector code-review gate

Date: 2026-08-18
Seat: Inspector (independent evaluation; not the builder seat)
Task: `01a0074f-672c-7f70-8bdd-da707dcda0ce` · dispatched TeamRun `01a010b4-4026-7382-a50c-e4a9f88b4d02`
Realm: `01a00649-9ee6-73e0-ba1b-6a6c35cfd065` · Project: `01a0064a-e056-7603-9968-ef64fdaacb75`

## Verdict

**REJECTED — changes requested.**

Two independent grounds, either sufficient on its own:

1. **Materially incomplete.** The delivered change is checkpoint 1 of 6. The
   entire Jira half of OP-07 — the clause the ticket is *named* for — is absent:
   no native connector, no connector configuration, no ASMA Epic binding rule.
2. **Two defects in what *was* delivered**, one of which puts a project into a
   durably unrecoverable state and directly contradicts an acceptance clause.

The work that was delivered is, on its own terms, of high quality: the schema
makes the illegal states unrepresentable rather than merely discouraged, the
deviations from the handoff are disclosed with reasons, and every declared gate
is green. This is an incomplete and partly defective ticket, not a careless one.

## Authority used

Authoritative plan:
`_docs/ai-orchestration/plans/2026-08-14-23-21-plan-kontor-operational-mvp.md`,
§ `### KON-OP-07`, verified **byte-identical** between superproject `master`
(`da86706f`) and this worktree — so no plan-drift caveat applies to this review.

Epic **Completion Profiles could not be read**: `kontor_completion_profiles_list`
answers `503 unavailable — "the Completion service is not composed in this
build"`. The verdict below is therefore made against the plan's Acceptance and
Verification clauses only. No Completion-profile predicate was assumed, invented
or waived.

## Exact code-change reviewed

| Item | Value |
| --- | --- |
| Superproject branch | `feat/ASMA-7876-kontor-jira-policy-cutover` |
| Superproject HEAD | `27814691` |
| Superproject gitlink → `_tools/asma-rs-kontor` | **`65412ba`** (docs only) |
| Submodule branch | `feat/ASMA-7876-kontor-jira-policy-cutover` |
| Submodule HEAD | **`3b95141`** — `feat(asma-7876): Own memory authority per project and subject` |
| Range reviewed | `65412ba..3b95141` — 1 commit, 26 files, +2463 / −131 |

`65412ba` (`docs(kontor): record OP-07 architecture handoff`) adds
`ARCHITECTURE.md` and nothing else. All reviewed code is in `3b95141`.

## Gates executed by this seat

Every gate `CONTRIBUTING.md` names, run on the delivered tree:

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | **pass** |
| `cargo clippy --workspace --all-targets -- -D warnings` | **pass** (exit 0, 0 warnings) |
| `cargo test --workspace --no-fail-fast` | **pass** (exit 0, 0 failed) |
| `pnpm --filter kontor-console verify:api` | **pass** (contract ↔ `schema.d.ts` clean) |
| `pnpm -r typecheck` | **pass** |
| `pnpm -r test` | **pass** (14 files, 278 tests) |

Green gates are not the finding. Findings 1 and 2 are outside what these gates
assert.

---

## Finding 1 — the import is not atomic across items and manifest, and cannot resume

**Severity: high. CONFIRMED by a runnable probe.**

`SqliteStore::apply_agentsroom_import` (`crates/kontor-store/src/memory.rs:599`)
commits items, revisions and approvals in one transaction, then calls
`record_subject_import`, which opens **its own** transaction for the manifest and
receipt. The two are not atomic.

`preview_agentsroom_import` computes `already_imported` from the manifest tables
**alone** (`memory.rs:564`). So if the second transaction fails, the durable state
is: items imported, no manifest — and the retry path believes nothing was
imported, re-runs the item loop, and dies on the primary key that is already
there. `memory_items` is inserted with a bare `INSERT`, no `ON CONFLICT`.

The project is then permanently stuck: the switch refuses forever
(`"the final hashed export has not been imported"`), and every re-import fails
identically. Exit requires manual database surgery.

This contradicts, directly:

- the plan's Acceptance clause — *"partial failure resumes only missing effects"*;
- `CHECKPOINT-01.md`'s own claim — *"injected failure rolls back items, revisions
  and the manifest together"*.

**Why the suite does not catch it.** The existing test
`cutover_is_attested_hashed_transactional_and_idempotent` (`memory.rs:1159`)
injects its failure with a trigger on `memory_items` — i.e. *inside* the first
transaction. It proves that transaction rolls back atomically. It never exercises
the window between the two transactions, which is the only window where the
non-atomicity is observable.

**Proof.** A probe was added temporarily to `mod tests`, run, and reverted (the
worktree is back to the delivered state). It fails the manifest insert only:

```rust
store.connection.execute_batch(
    "CREATE TRIGGER fail_manifest BEFORE INSERT ON subject_import_manifests
     BEGIN SELECT RAISE(ABORT, 'injected manifest failure'); END;")?;
assert!(store.apply_agentsroom_import(&export).is_err());
// PROBE: the items survived the manifest failure
assert_eq!(items_count, 1);
// PROBE: the retry still believes nothing was imported
assert!(!store.preview_agentsroom_import(&export)?.already_imported);
// PROBE: the retry fails on the duplicate item
assert!(store.apply_agentsroom_import(&export).is_err());
// PROBE: imported, unmanifested, unswitchable
assert!(store.switch_project_memory_authority(..).is_err());
```

Result: `test memory::tests::review_probe_import_is_not_atomic_across_items_and_manifest ... ok`
— every assertion held.

**Required to pass.** Either record the manifest inside the same transaction as
the items (the readback can be computed from the uncommitted transaction's own
rows), or make the item loop idempotent and derive `already_imported` from durable
state rather than from the manifest alone. Add the between-transactions failure to
the test, not just the within-transaction one.

---

## Finding 2 — backlog authority is advertised through `/v1` but enforced nowhere

**Severity: high. CONFIRMED by inspection; reachable through the public admin API.**

`projects:ensure` accepts `backlog_origin: "legacy_pending"` and nothing rejects
it (`crates/kontor-daemon/src/applications.rs:4705`; no validation anywhere —
`grep -rn legacy_pending crates/ --include=*.rs` finds no refusal path). The
resulting row is reported as authoritative by two public surfaces:

- `GET /v1/projects/{id}/subjects/authority` → `backlog.authority = "agentsroom"`
- the `projects:ensure` response body itself → `backlog: { authority: "agentsroom" }`
- and MCP `kontor_subject_authority_get`.

But `require_subject_authority` has exactly four call sites, all
`AuthoritySubject::Memory` (`memory.rs:178, 245, 451, 488`). **No backlog write
path checks it.** Kontor will create epics, tasks, dependencies and lifecycle
state for a project whose ledger says AgentsRoom owns its backlog.

`CHECKPOINT-01.md` discloses this as deferred to checkpoint 5. Disclosure is the
right behaviour and is noted — but the deferral is not neutral here, because the
change *ships the claim* while deferring the enforcement. An operator reading the
endpoint has been told writes are refused when they are not. That is the
dual-writer condition the plan forbids in terms
(*"Never run old and new effect writers concurrently"*), and it is the acceptance
clause *"no legacy path can produce a second authoritative write for that
project/subject"* read from the other side.

**Required to pass** — either is acceptable:

- gate the backlog write paths now; or
- refuse `backlog_origin: "legacy_pending"` at `projects:ensure` until checkpoint 5
  lands, so the unenforced state is unrepresentable rather than merely undocumented.

---

## Finding 3 — the Jira half of OP-07 is absent

**Severity: high (scope). CONFIRMED.**

Verified present and unchanged in the delivered tree:

- `crates/kontor-integrations-asma/src/jira.rs:811` — `TicketDelegation` still
  holds `pub asma: &'a AsmaExecutable`. The subprocess is still the one writer.
- `crates/kontor-integrations-asma/{process,jira,lib}.rs` — `AsmaExecutable` intact.
- `crates/kontor-daemon/Cargo.toml:45` — `kontor-integrations-asma` is still a
  daemon dependency.
- No `kontor-jira` crate exists (`ls crates/`).

Unmet Acceptance clauses, quoted from the plan:

- *"`kontord` creates, links, observes, applies and refetches Jira with `asma` absent"*
- *"no Kontor code invokes `asma jira sync --request-json -`, and that legacy mode
  forwards or refuses rather than writing Jira directly"*
- *"Human Jira/import, Git/ACP transition and prompt-write modes return Kontor
  receipts or fail without the semantic effect"*
- *"An ASMA Epic cannot activate without one Jira Epic binding and its delivery
  Tasks cannot execute without Jira Issue bindings"*
- *"Preview bytes/hash match apply input; repeated apply creates no duplicates;
  conflicting existing key/parent/field blocks; ... Zone C and absent fields never
  leak"* — none of this exists natively yet.

Roughly half of OP-07's acceptance surface is untouched. `CHECKPOINT-01.md` states
this plainly; the reviewer's job is only to record that it is not deliverable scope
that can be gated as passing.

---

## Finding 4 — published contract declares a status the endpoint never returns

**Severity: low. CONFIRMED.**

`POST /v1/memory/cutover:freeze` is annotated `responses((status = 409))`
(`crates/kontor-api/src/memory.rs`), and that is what
`crates/kontor-api/contract/openapi.json` publishes. The handler returns
`ApiErrorCode::InvalidRequest`, which maps to **400**
(`crates/kontor-api/src/error.rs:102`) — and the delivery's own test asserts 400
(`crates/kontor-daemon/tests/loopback_api.rs:2147`).

`verify:api` cannot catch this: it diffs the generated `schema.d.ts` against the
committed one, so a contract that is internally consistent but untrue still passes.
A generated client branching on documented statuses gets this endpoint wrong.

**Required to pass.** Change the annotation to `400`.

---

## Finding 5 — the delivery is not reachable from the superproject

**Severity: medium (process, not code). CONFIRMED. This is why the gate saw nothing.**

- Submodule HEAD `3b95141` is **not pushed**: `git branch -r --contains 3b95141`
  is empty. It exists only in this local worktree.
- The superproject gitlink still points at `65412ba` — the docs-only handoff. The
  code commit is invisible to `asma-modules`.
- `.gitmodules` sets `ignore = all` for `_tools/asma-rs-kontor`, so the
  superproject reports a **clean** tree despite the drift. `git show`/`git status`
  on the pointer commits look empty; only tree-hash comparison reveals them
  (`27814691` tree `dadea3d` ≠ `1c9e88b2` tree `cc0e502`).

This fully explains the reconciliation notice. There was no code-change evidence to
find from the superproject's point of view, because from that point of view OP-07
has so far delivered one Markdown file.

**Required.** Push the submodule branch and advance the superproject gitlink, so
the code-change ref this review cites resolves for anyone but this machine.

---

## What is accepted as sound

Recorded so the rebuild does not discard it:

- **`0032_project_subject_authority.sql`** — the two table CHECKs make an
  empty-export ceremony and a partially-switched row *unrepresentable*, not merely
  refused in application code; the guarded-update trigger permits exactly the
  attestation and the switch; the singleton is frozen rather than dropped, keeping
  the evidence of what the realm used to claim. The existing-project seed correctly
  distinguishes backlog (native — the graph was always Kontor's) from memory
  (inherits the singleton's claim), and `memory_authority` is guaranteed non-empty
  by `0021_native_memory.sql:102`, so the seed JOIN cannot silently drop rows.
- **`kontor_core::authority`** — separating immutable `SubjectOrigin` from mutable
  `SubjectAuthority` is the distinction the whole design rests on, and it is drawn
  in the type system.
- **`require_subject_authority` runs inside the caller's transaction**, so the check
  and the write it guards commit or roll back together. The doc comment states the
  reason. Correct.
- **`memory_readback_hash` is computed from stored rows**, never from submitted
  bytes, and the switch refuses a recomputation that disagrees with the manifest.
- **Deviation 1 (`legacy_pending`) is legitimately forced.** Verified: the
  checked-in guard `tests/contract/mcp_mutants.rs` audits enum values as well as
  tool and argument names (`every_identifier()`, line 30), and `agentsroom` is a
  forbidden needle (line 157). `agentsroom_import_pending` would trip it. Renaming
  rather than exempting a deliberate guard was the right call. Note the plan text
  names the value explicitly, so the epic owner should ratify the vocabulary change
  even though the concept is unchanged.
- **Deviations 2–5 are disclosed with reasons and are individually defensible.**

## Summary of what a passing resubmission needs

1. Fix Finding 1 (atomic or resumable import) — with a test in the window that is
   currently untested.
2. Fix Finding 2 (gate backlog writes, or refuse the unenforceable declaration).
3. Fix Finding 4 (one-line annotation).
4. Push the branch and advance the gitlink (Finding 5).
5. Deliver checkpoints 2–6, or have the epic owner formally re-scope OP-07 so that
   the Jira clauses move to their own ticket. **This is not the reviewer's call to
   make** — as written, the acceptance criteria are not met.

---

## Control-plane record

| Item | Value |
| --- | --- |
| Gate | `code-review-gate` (from the pinned `code` work profile v1) |
| Verdict | **`rejected`** |
| Evaluator role | `inspector` (the profile's only permitted evaluator for this gate) |
| Evaluator account | `01a00751-5be9-7281-bba5-75d8c0c101e7` — Igor · Local Paseo |
| Evidence cited | `code-change`, `review-notes` |
| Workflow revision | `1` |
| Receipt | `01a01397-f939-7422-81a7-4c03b5622785` (sequence 1) |
| Read back | `kontor_task_get` → `gates: { "code-review-gate": "rejected" }` |

Evidence resolution:

- **`code-change`** → submodule `asma-rs-kontor` commit `3b95141`, range
  `65412ba..3b95141`, branch `feat/ASMA-7876-kontor-jira-policy-cutover`.
  Not yet pushed; see Finding 5.
- **`review-notes`** → this document,
  `_tools/asma-rs-kontor/docs/evidence/KON-OP-07/CODE-REVIEW-01.md`.

The profile sets `waiver_allowed: false` on this gate, so the rejection cannot be
waived — it routes to `implementation` and must be re-earned.

Two notes for the orchestrator:

1. `kontor_run_get` reports **404 `not_found`** for the dispatched run
   `01a010b4-4026-7382-a50c-e4a9f88b4d02` — no such agent run exists in this realm.
   The verdict was therefore recorded against the *task*, which is where the gate
   lives. If a run-scoped record is also wanted, the correct run id is needed.
2. `kontor_completion_profiles_list` reports **503** — the Completion service is not
   composed in this build — so the epic Completion Profiles named in the dispatch
   could not be read or evaluated. This review is against the plan's Acceptance and
   Verification clauses only, and no Completion predicate was assumed.

---

# Re-review — remediation 1

Date: 2026-08-18
Reviewed: `3b95141..d3ecbec` — 1 commit, 14 files, +577/−109
Superproject: `bcf96567` (pointer-only; verified it changes the gitlink and nothing else)

## Verdict

**PASSED.** Receipt `01a013d4-6056-7542-9378-bb5c34332d9a` (sequence 2),
workflow revision 1, evidence `code-change` + `review-notes`. Read back:
`gates: { "code-review-gate": "passed" }`.

## Epic-owner decisions applied

Recorded as binding and applied to this verdict:

1. **Finding 3 (Jira half) re-scoped** to its own ticket — not held against OP-07.
2. **`legacy_pending` vocabulary ratified** — the deviation stands.
3. **Push authorized and done** — Finding 5 cleared, verified independently:
   `origin/feat/ASMA-7876-kontor-jira-policy-cutover` contains both `d3ecbec`
   (submodule) and `bcf96567` (superproject); the gitlink now resolves.

## Findings 1, 2, 4 — verified fixed, and the tests verified load-bearing

Claims were not taken on trust. Each fix was read, and each new test was
mutation-checked by this seat.

**Finding 1 — import atomicity. Fixed.** The item loop, the readback and the
manifest are now one transaction; `subject_authority_in` reads the origin inside
it, and `memory_readback_hash` was hoisted to a free function over `&Connection`
so the hash is computed from the rows this transaction wrote. Leaving the FTS
rebuild outside is correct and correctly reasoned — it is a derived projection.

> **Mutation applied:** restored the commit between items and manifest.
> `an_import_that_fails_while_recording_its_manifest_leaves_nothing_and_resumes`
> → **FAILED**, `left: 2, right: 0` on `memory_items`. The builder's reported
> mutation result reproduces exactly.

**Finding 2 — backlog authority. Fixed, both halves.** `require_backlog_authority`
guards `apply_epic` and `transition_task` inside their existing transactions, via a
new `RepositoryError::AuthorityWithheld` mapped to `forbidden` — correctly *not* a
conflict, since re-reading withheld authority returns the same answer. And
`projects:ensure` now refuses `backlog_origin: legacy_pending` (400
`invalid_request`), with the MCP enum narrowed to `BACKLOG_ORIGINS`. Refusing a
state that no operation could clear is the right call, and it closes the hazard at
the source: no supported surface can now create a `legacy_pending` backlog.

> **Mutation applied:** removed the guard from `apply_epic`.
> `a_backlog_a_legacy_system_owns_refuses_the_writes_that_are_the_backlog`
> → **FAILED**, `left: 200, right: 403` — and the mutant response body shows the
> epic and its task being created. The guard is load-bearing.

**Finding 4 — contract/route status. Fixed.** The contract now declares `400`, and
the test asserts the *declared status set equals the returned status* rather than
asserting the code alone. That fixes the class, not just the instance.

## Gates — re-run independently by this seat, on `d3ecbec`

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | pass (0) |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass (0) |
| `cargo test --workspace --no-fail-fast` | pass (0) — **1401 passed, 0 failed** |
| `cargo deny check` | pass (0) |
| `pnpm --filter kontor-console verify:api` | pass (0) |
| `pnpm -r typecheck` | pass (0) |
| `pnpm -r test` | pass (0) — 278 |

Every figure in `REMEDIATION-01.md` reproduces.

Also noted with approval: `CHECKPOINT-01.md`'s false atomicity claim was corrected
in place rather than quietly dropped, with a pointer to the remediation.

## New finding — non-blocking, does not withhold this gate

**`transition_task`'s backlog guard has no test coverage.**

`REMEDIATION-01.md` says the guard covers "the two writes that *are* the backlog",
and the test's own comment says "Moving a task's lifecycle is the same subject, so
it refuses the same way". The test does not make that call — it reads the authority
projection instead. Nothing anywhere exercises a lifecycle transition against a
legacy-owned backlog.

> **Mutation applied:** removed `require_backlog_authority` from `transition_task`
> entirely, then ran the whole workspace suite. **1401 passed, 0 failed, exit 0.**
> The guard is completely uncovered.

The code is correct — this is a proof gap, not a defect, which is why it does not
withhold the gate. But it is the same class of overstatement that produced Finding
1 (a document asserting a property no test held), so it is recorded rather than
waved through. Add the lifecycle call to the existing test; the fixture that seeds
the pending-backlog project is already there.

**Related, for checkpoint 5.** `require_backlog_authority` guards two paths, but
these also write backlog state and are unguarded: `create_mini_project`,
`create_task` and the dependency insert (`repository.rs:478/502/588`), the intake
writers (`intake.rs:718/732`), and the task UPDATE in `policy.rs:1146`. Harmless
today — `legacy_pending` backlogs are unrepresentable through every supported
surface — but the moment checkpoint 5 makes that origin declarable again, partial
enforcement becomes a live dual-writer hazard. Guard them in the same change that
lands the backlog import.

## Handover note

`current_phase` is still `implementation` and the QA gate is `not_ready`. The
code-review gate is satisfied; advancing the phase is the orchestrator's call, not
this seat's.

---

# Evidence reconciliation — 2026-08-19

Recorded by the inspector seat at the epic owner's instruction to reconcile the
evidence gaps. Nothing here reopens the `code-review-gate`.

**Read this section against a moving target.** The worktree was under concurrent
modification by other seats throughout this reconciliation: a modified test file
and `FOLLOW-UP-01.md` appeared mid-turn, and commit `fddaa66` plus superproject
`8155810b` landed after that. Every claim below is pinned to a named commit rather
than to "the worktree", because "the worktree" was not a stable object.

## Why this file is committed as of this revision

It was not, and the release notes cited it anyway. `6f977ad`'s release-notes report
lists `docs/evidence/KON-OP-07/CODE-REVIEW-01.md` under **Evidence References**,
while the file existed only as an untracked working-tree artifact. A clone at that
commit got a release document citing evidence the repository did not contain.

The gate verdicts were always durable in Kontor — receipts
`01a01397-f939-7422-81a7-4c03b5622785` (rejected) and
`01a013d4-6056-7542-9378-bb5c34332d9a` (passed) — so nothing was ever inferred
from this file. But the reasoning behind them was not reproducible from the
repository, which is what an evidence reference is for.

## The gate passed at `d3ecbec`; the shipping tree is now `fddaa66`

Re-verified rather than assumed, because a gate verdict names a tree:

| Range | Contents | Runtime source touched |
| --- | --- | --- |
| `d3ecbec..6f977ad` | `QA-01.md`, release-notes report | none |
| `6f977ad..fddaa66` | `Cargo.lock` only, 2 lines: `h2` 0.4.15 → 0.4.16 | none |
| uncommitted | one added test, `FOLLOW-UP-01.md` | none |

No runtime source has changed since the tree the `code-review-gate` passed
against. Verified with `git diff --stat` restricted to `crates/`, `apps/` and
`tests/` across both ranges — both empty.

Re-run on the current tree by this seat: `cargo test --workspace --no-fail-fast`
→ **1402 passed, 0 failed** (the extra test over the 1401 at `d3ecbec` is the new
regression below), and `cargo deny check` → **pass**. The verdict stands on the
tree that is actually shipping.

### The `h2` advisory, since it postdates the gate

`RUSTSEC-2026-0258` (`h2` 0.4.15 accepts and queues empty DATA frames without
limit; low severity, patched in 0.4.16) reaches this workspace transitively via
`axum 0.8.9 → hyper 1.11.0 → h2`. It did not exist when this seat ran
`cargo deny check` at `d3ecbec` on 2026-08-18 — that run passed. `fddaa66` bumps
the lock and `cargo deny check` passes again. Recorded because a reader comparing
the two `deny` results across dates would otherwise see a contradiction.

## The accepted proof gap is closed — verified, not taken on trust

`FOLLOW-UP-01.md` and
`a_task_under_a_legacy_owned_backlog_refuses_its_lifecycle_transition`
(`crates/kontor-store/tests/repository_roundtrip.rs`, uncommitted at the time of
writing) close the `transition_task` coverage gap this review raised.

Independently checked by this seat:

- The test **passes** on the current tree (1/1).
- **Mutation applied:** `require_backlog_authority` removed from `transition_task`
  → the test **FAILED**, with the task observably moved to `InProgress` at
  revision 2. Previously that same mutation left all 1401 tests green.

The gap is genuinely closed. The fixture correctly builds its task through
`create_mini_project`/`create_task` rather than `apply_epic` — which now refuses
such a project outright and so cannot produce the fixture at all.

`FOLLOW-UP-01.md`'s write-seam audit also supersedes the checklist this review
left behind, and improves on it: it reaches seams this review did not name
(`replace_task_workflow`, `park_task`, `apply_recovery_transition`,
`write_transition`, the scheduler's run inserts) and recommends deciding the guard
at the single transaction boundary they share rather than site by site. That is the
better answer. This review's own list should be read as superseded by it.

## Gate provenance — checked, and clean on authority

The concern was that `qa-gate` and `release-gate` read `passed` while `QA-01.md`
says in terms: *"QA evidence passes; QA gate settlement is blocked on Kontor
control-plane recovery"* and *"Do not infer a passed QA gate from this file."*

Read back through `kontor_run_get`:

| Run | Role | Gate its role owns | Lifecycle | `closed_at` | `outcome` | `gaps` |
| --- | --- | --- | --- | --- | --- | --- |
| `01a01385-d87b-7be2-b3c9-6e19a0c47ecb` | `tester` | `qa-gate` | `queued` | `null` | `null` | `[]` |
| `01a01926-c870-7d21-9aa4-005380582d43` | `architect` | `release-gate` | `queued` | `null` | `null` | `[]` |

Both seats hold exactly the role the pinned `code` profile requires as that gate's
evaluator. **No authority violation is evident**, and the daemon that was
unreachable on 2026-08-18 is reachable now, so a legitimate post-recovery
settlement is the reading the evidence supports. `QA-01.md`'s warning describes the
moment it was written, not the moment the gate was recorded.

Kontor serves no tool at this authority that reports *which* principal recorded a
gate verdict, so this is a role-and-consistency check, not an attribution proof.
Stated as such rather than stronger.

## What is genuinely unreconciled

**1. Both operational-gap records exist only as repository markdown.** The `gaps`
array is empty on both runs. The QA gap report requires, at step 3, *"Attach this
report as typed `operational_gap` evidence to the existing OP-07 task"*, and at
step 5 *"Record the recovery receipt here or in Kontor"*. Neither was done on
either side: both reports are still `🟡 In Review`, and Kontor holds no gap for
either run. No gap-recording tool is served at this seat's authority, so this is
recorded rather than performed.

- `_docs/ai-orchestration/reports/2026-08-18-09-53-report-kontor-op07-qa-control-plane-gap.md`
- `_docs/ai-orchestration/reports/2026-08-19-10-40-report-kontor-op07-timeline-refetch-gap.md`

The second is a live product defect, not bookkeeping: an unbounded
`timeline_refetch_required` loop returning no cursor or revision. Its step 2 asks
for the smallest regression proving a refetch yields a stable page or one typed
terminal refusal. That is unwritten.

**2. Neither run is closed.** Both are `queued` with `outcome: null` and
`closed_at: null` — consistent with the timeline-refetch gap, since the runtime
history those seats needed was never reachable.

**3. `current_phase` is still `implementation`** with all three gates passed and
the task `in_progress`, though the profile's terminal phase is `release`.

**4. The follow-up test and `FOLLOW-UP-01.md` are uncommitted** as of this
writing, by a seat other than this one. They are that seat's to commit; this
commit deliberately stages only this document.

None of the above is the inspector's to settle: gap attachment belongs to the
seats that raised them, run closure and phase advance to the orchestrator, and the
timeline-refetch repair to the Kontor runtime owner. They are recorded so the
ticket does not read as fully closed while four control-plane obligations are
outstanding.
