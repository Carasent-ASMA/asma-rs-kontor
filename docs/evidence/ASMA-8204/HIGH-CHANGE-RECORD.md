# ASMA-8204 high-change record: archive epic roots after evidence-backed closeout

Date: 2026-09-17
Artifact: `high-change`
Task: `ASMA-8204` / `01a0ac9d-a96e-7de1-abfe-3d340ec5fea4`
Project: `01a0064a-e056-7603-9968-ef64fdaacb75`
Phase: `high-implementation`
Branch: `feat/ASMA-8204-archive-epic-roots-after-evidence-backed-closeout`
Base: `2e4b9985`
Status: first implementation turn against the settled scope

## This turn

The change [`HIGH-SCOPE-RECORD.md`](HIGH-SCOPE-RECORD.md) authorizes, implemented
whole against its bounded-implementation section and proved against its
required-verification section. No new API, no new MCP tool, no schema change, no
migration, no deployment, no merge, no Jira mutation, and no live runtime state
touched.

The scope's shape held under implementation: the existing seat and node
lifecycle operations already owned revision checks, idempotency receipts,
retry and readback, and the whole defect really was at their shared archive
boundary. Nothing here adds a cascading `topology:closeout` route.

## The change, layer by layer

### `kontor-runtime` — one request shape for both containers

`ArchiveContainerRequest::bound_project_native_id` becomes
`Option<ExternalId>`, matching the precedent `RetitleContainerRequest` already
set in the same module. The pairing rule lives in one new accessor,
`ArchiveContainerRequest::parent_project`:

- `NativeChild` + `Some(parent)` → the parent;
- `NativeRoot` + `None` → no parent;
- every other pairing, including `LogicalOnly`, refuses.

Putting it there rather than in each adapter is what makes "a root cannot name
an ancestor it does not have" a property of the request instead of a
convention two adapters are each trusted to remember. Both adapters call it
before they look anything up, so a contradictory request never reaches a
census, let alone an effect.

### `kontor-runtime` fake — the ordering it will be asked to prove

The fake now accepts a root, and refuses one whose native children are still
unarchived. That refusal is not decoration: the loopback regression drives its
epic closeout through this adapter, so without it the fake would happily let a
parent-first closeout settle in the very test written to forbid it.

### `kontor-runtime-paseo` — one preamble, two effects

`archive_container` keeps a single shared preamble — session permissions, the
shape's admissible ancestry, runtime host and generation, operator adoption,
agreement with the registered binding — and then branches into
`archive_bound_child` (the previous body, unchanged in behaviour) and the new
`archive_bound_root`.

`archive_bound_root` gates in the order in which being wrong costs least:
exact-id selection, canonical root directory, zero remaining workspaces, zero
unarchived sessions, then the advertised `projectRemove` capability. Only then
does it send `project.remove.request` addressed by exact `projectId`.

The acknowledgement is discarded, exactly as the child path discards the CLI's.
A daemon that removed the project and lost the reply is indistinguishable on the
wire from one that refused; only the fresh complete project listing afterwards
can tell those apart, so that listing is the sole evidence and a retry after
prior absence reports `changed = false`.

Supporting surface, kept to what clause 7 needs: `PaseoFeature::ProjectRemove`
(`projectRemove`, advertised by the supported 0.8.0 baseline) and
`PaseoRpc::project_remove`. `ProjectRemove` is deliberately **not** added to
`REQUIRED_FEATURES` — it gates one operation at its point of use, the way
`ProjectRename` does, rather than raising the floor for driving Paseo at all.

### `kontor-store` — leaves before roots, where every caller passes

`transition_topology_node` refuses an archive while any direct child is short of
`archived`. It sits beside the existing retire guards, in the one transaction
every route into the lifecycle shares, so the ordering cannot be re-decided by
whichever caller happens to run.

### `kontor-daemon` — shape resolution and the completion gate

`archive_native_child` becomes `archive_native_container` and resolves the
projection from the node's persisted binding exactly as the retitle route
already does (`Project` → root, `Workspace` → child), walking the ancestry for a
child only. Its pre-existing guards — retired node, no active seats or children,
no open TeamRun — are untouched and now cover roots too.

`ensure_root_completion_done` is the new gate. It resolves the projection from
the node's **own pinned specification revision** rather than from what it
currently holds, so a declared root that never materialized is gated like one
that did, and it admits exactly `CompletionPhase::Done`. A missing run refuses
as `NotFound` and an undecodable one as `Unavailable`, from the existing
accessors; neither is read as "nothing to check". It runs on both terminal
directions, because gating only the archive would let an incomplete epic lose
its native place in two steps instead of one.

## Where each closeout clause is enforced

| Clause | Enforced at | New? |
|---|---|---|
| 1 existing operations authoritative | `move_node_lifecycle`, existing seat/node routes | no |
| 2 child-to-root order enforced | `transition_topology_node`; adapters repeat occupancy | **yes** |
| 3 delivery work closes first | `archive_native_container` → `list_open_team_runs` | no |
| 4 seats precede their host | `transition_topology_node` retire guards | no |
| 5 leaves precede roots | as clause 2, plus `archive_bound_root` emptiness | **yes** |
| 6 completion is the root gate | `ensure_root_completion_done` | **yes** |
| 7 ESW cleanup is physical | `archive_bound_root` | **yes** |
| 8 lost acknowledgement replay-safe | discarded ack + absence readback | **yes** |
| 9 adopted and project-wide roots retained | adopted check in the shared preamble; unscoped nodes already refused by `move_node_lifecycle` | no |
| 10 identity evidence survives cleanup | nothing deleted; binding equality asserted after archive | no |

## Scope decisions recorded

**Protocol, not CLI.** The scope cited both `paseo project delete` and
`project.remove.request` as evidence that root cleanup has a supported physical
effect. The RPC is what was implemented, because it is what the scope names as
"the supported protocol operation", it is addressed by exact `projectId`, and it
is the surface every other project operation in this adapter already uses.

**OQ-002 is new and recorded.** Clause 7's "zero unarchived sessions" is not
directly answerable on Paseo 0.8.0 — an agent carries no project id — and the
obvious formulation is vacuous once zero workspaces is required first. The
disposition and its residual risk are in
[`OPEN-QUESTIONS.md`](OPEN-QUESTIONS.md).

## Verification

Every check below was run on this branch at `2e4b9985` plus this work.

### The scope's required verification

| # | Required | Where | Result |
|---|---|---|---|
| 1 | closeout in exact order; early ECP/ESW refusal with zero root effect; missing/non-done completion refuses; `Done` permits | `an_epic_root_archives_last_and_only_when_its_completion_is_done` | pass |
| 2 | lost ESW archive ack, same key retried, one removal, one receipt, same binding, archived after readback | same test | pass |
| 3 | exact-id/cwd, adopted, occupied, capability, post-remove absence, retry after absence | `native_root_removal_*` (3 tests) | pass |
| 4 | a retired-but-unarchived child blocks parent archive | `a_retired_but_unarchived_child_blocks_its_parents_archive` | pass |
| 5 | MUT-009 killed twice, both reverted | below | pass |

Requirement 1 additionally covers a case the scope names but does not enumerate:
`NeedsHuman` is terminal and still does not permit root retirement.

### Mutation results

Eight mutants, one at a time, each seeded, run against the narrowest suite that
claims to cover it, reverted, and the tree re-verified. The two the scope names
are MUT-009a and MUT-009b.

| ID | Defect seeded | Killer | Result |
|---|---|---|---|
| MUT-009a | drop the child-archive guard (store) | `a_retired_but_unarchived_child_blocks_its_parents_archive` | killed |
| MUT-009b | restore the prior "native root archive is not supported" refusal | `an_epic_root_archives_last_and_only_when_its_completion_is_done` | killed |
| MUT-8204-c | bypass the completion gate entirely | same | killed |
| MUT-8204-d | admit any terminal phase, not only `Done` | same | killed |
| MUT-8204-e | drop the root's zero-workspace check | `native_root_removal_refuses_unsafe_targets_before_any_mutation` | killed |
| MUT-8204-f | trust the removal ack; skip the absence readback | `native_root_removal_does_not_trust_an_ack_without_absence` | killed |
| MUT-8204-g | let a root name a parent project | `native_child_archive_refuses_unsafe_targets_before_any_mutation` | killed |
| MUT-8204-h | drop the advertised `projectRemove` check | `native_root_removal_refuses_unsafe_targets_before_any_mutation` | killed |

### What mutation testing caught that review did not

**A vacuous assertion, and the reason it read as sound.** The loopback
regression originally probed "a retired child blocks its parent's archive"
*before* the ESW had retired. MUT-009a survived it. The probe was being answered
by `archive_native_container`'s "native cleanup requires a retired topology
node" gate, so it passed without the child invariant ever being reached — the
test asserted a true thing for a reason that had nothing to do with the code
under test. The probe now runs after the ESW retires, its seats are gone and its
completion is done, which is the only state where the unarchived child is the
sole possible refusal.

That correction exposed a second fact worth recording: at that point the
refusal arrives from the *runtime's* occupancy check rather than the store's,
because native cleanup runs before the logical transition. Both layers refuse,
by design. The end-to-end probe therefore deliberately does not assert which one
answered; each guard is pinned where it is attributable — the store by the store
test, the adapter by the Paseo contract's `occupied-by-a-workspace` case — and
the loopback asserts the flip instead: the identical request settles once the
child, and nothing else, has been archived.

**A stale-build trap that made three reverts look clean when they were not.**
Reverting a mutant with `cp`/`mv` restored the file with an *older* mtime than
the mutated build, so cargo skipped the rebuild and kept the mutant in the test
binary. `git status` and `git diff` were both clean while two contract tests
failed. Every mutant was re-run with an explicit `touch` after restore, and the
recorded results above are from that second, trustworthy pass. The pre- and
post-pass working trees are identical.

### A regression this turn introduced, and then removed

The completion gate initially ran before the node's structural checks, so
retiring an ESW that still had a live ECP answered `404 this epic has no
completion run yet` instead of the `409` the children rule had always produced.
That broke `a_node_is_retired_by_the_id_an_answer_returned_and_children_block_it`
— a test whose own comment says an early refusal for the wrong reason would
prove "nothing about children blocking a parent". It was right.

The gate now defers while the node still hosts active seats or children: in that
state the structural refusal is both more specific and the one callers already
depend on, completion has nothing to add, and nothing advances either way
because the store refuses the transition regardless. Clause 6 is unaffected —
no transition and no runtime effect can occur without a `Done` completion.

The active-work read is now one helper, `hosts_active_work`, shared by the
archive route and the completion gate so the two cannot drift apart about what
"still busy" means. MUT-8204-c, MUT-8204-d and MUT-009b were re-run against the
corrected gate and are still killed.

**This was found only by running the whole `loopback_api` suite.** The focused
filter this turn had been using (`an_epic_root_archives_last`) ran one test and
never saw it.

### Gates

| Gate | Result |
|---|---|
| `cargo fmt --all -- --check` | clean for every file this work touches; one pre-existing violation elsewhere (below) |
| `cargo clippy --workspace --all-targets -D warnings` | passed |
| `cargo test --workspace --locked --no-fail-fast` | **2491 passed, 10 failed** across 140 suites; all ten proved pre-existing below |

`--no-fail-fast` matters and is not cosmetic: without it cargo stops at the
first failing target, and the first run of this suite reported "6 suites, 147
tests" while 134 suites had never been reached.

**Reconciliation against the run taken before the precedence fix:**

| Run | Passed | Failed |
|---|---|---|
| before the fix | 2490 | 11 |
| after the fix | 2491 | 10 |

The delta is exactly one test —
`a_node_is_retired_by_the_id_an_answer_returned_and_children_block_it`, the
regression described above. No other result moved in either direction.

## Pre-existing defects at the base commit, proved by a clean-tree run

`2e4b9985` was checked out into a separate detached worktree carrying none of
this work, and the same targets were run there. Every failure below reproduces
**identically** on that clean tree, so none of them is caused by this work.

The last three are additionally confirmed fixed on `origin/master` (`e19ccd3d`),
which this base is seven commits behind. The `loopback_api` and `mcp_journey`
failures were proved pre-existing at this base but were **not** re-run against
`origin/master`, so this record makes no claim about whether integration clears
them — the next seat should expect to reconcile them rather than assume they
disappear.

| Defect | Clean-baseline evidence |
|---|---|
| 8 `loopback_api` failures — Jira-connector `503`, placeholder-body and admission-replay cases | baseline: `335 passed; 8 failed`, the **same eight names**. This branch: `336 passed; 8 failed` — the extra pass is this work's new test |
| `an_empty_realm_is_bootstrapped_through_mcp_tools_alone` (`mcp_journey`) | fails identically on the clean baseline (`1 passed; 1 failed`); it refuses on backlog-code and dependency evidence, nothing this work touches |
| `the_committed_contract_document_is_the_one_this_crate_serves` | fails on the clean baseline too. The drift is `fill_team_run_seat` from `7eacdab3` (#225), added without regenerating the doc; the regenerated diff contains only that route and nothing from this work |
| `cargo fmt --check` — `crates/kontor-teams/tests/team_contract.rs` | 1 diff on the clean baseline; not a file this work touches |
| `kontor-tests-e2e` does not compile — `JiraResponse` missing `observed_identity` | 2 compile errors on the clean baseline; the field came from already-merged ASMA-8116 |

The e2e target is excluded from the runs reported here because it does not
compile at this base commit — that exclusion is the reason, not a judgement
about coverage.

The OpenAPI document was regenerated once solely to identify the drift's owner,
then reverted: the scope freezes that schema and the drift belongs to #225.

## Integration notes for the next seat

- **Rebase, do not cherry-pick.** The scope's live-repair boundary forbids
  cherry-picking ASMA-8201 into this branch, so this turn stayed on `2e4b9985`.
  Integration onto current `origin/master` resolves all three baseline defects
  above at once.
- **Watch ASMA-8199.** `22205970 ASMA-8199 ecp native materialization` landed on
  master and works on ECP native materialization — the same topology this change
  archives. It is the collision most likely to need reconciliation, and it was
  not visible to this turn.
- No migration slot is claimed, so there is no renumbering to reconcile.

## What this turn did not do

- No live runtime state was read or written. ASMA-8001 was not touched: the
  scope admits its repair only from a same-fleet deployed artifact that also
  carries ASMA-8201 `11fdc130`, which this isolated branch must not contain.
- No project was removed anywhere. Every `project.remove.request` in this record
  was issued against a recorded test double.
- The API, MCP registry, OpenAPI schema, completion machine and database schema
  are unchanged.

## Remediation of HV-001 (verification rejection of `0e5a0bec`)

Independent verification rejected candidate `0e5a0bec` with one P1, recorded as
approved Kontor artifact
`artifact-asma-8204-high-verification-report-0e5a0bec` revision 1
(`01a0b34f-e702-7973-8276-6ac7a86f97be`).

**HV-001 — OQ-002 filesystem-root containment false negative.** `within` decided
containment by stripping the root as a text prefix and requiring the remainder
to start with a separator. That is exactly right for `/w/epic` against
`/w/epic-2`, which it must reject, and wrong for the filesystem root: stripping
`/` from `/dangling-session` leaves `dangling-session`, with no leading
separator. Because `WorkspaceRoot` accepts `/` as a spellable place, an epic
root can legitimately be bound there — and a live unarchived session inside it
was then reported as outside it. The zero-unarchived-sessions precondition was
satisfied vacuously and the irreversible exact-id `project.remove.request`
proceeded. The verifier's own disposable probe observed `changed = true` where a
refusal was required.

The finding is accepted in full. It is a defect in the gate, not a residual: the
approved change artifact's OQ-002 text described the weakness as an
outside-the-tree association, which understated it.

**Correction.** Containment walks path components instead of comparing strings.
`/` is the single `RootDir` component every absolute path begins with, and
`epic` and `epic-2` are different components, so both cases fall out with no
special case for the root. The predicate still only ever *refuses*, and an
unparsable cwd still refuses into containment rather than out of it.

**Coverage, and its non-vacuity.** A unit test over the predicate covers exact,
descendant at one and several levels, the filesystem root, siblings sharing a
textual prefix, ancestors, unrelated branches, the trailing-separator spelling,
and five malformed spellings. `native_root_removal_refuses_a_live_session_under_the_filesystem_root`
keeps the verifier's probe as a contract regression: root `/`, zero workspaces, a
dangling unarchived session at `/dangling-session`, asserting both the refusal
reason and that nothing was mutated. MUT-8204-i restores the rejected prefix
logic and both seams fail — the unit test on the `/` case, and the contract probe
by reaching the removal, which is the defect itself.

All eight prior mutants were re-seeded and re-killed against this tree. Closeout
ordering, the completion gate, the lost-acknowledgement replay semantics and the
store invariant are unchanged by this remediation.

**A stale-build trap, again.** While re-verifying, `a_retired_but_unarchived_child_blocks_its_parents_archive`
failed deterministically against source that provably contained the guard, in
both the working tree and the committed head. The cached test binary predated a
mutant restore whose `cp` had regressed the file's mtime, so cargo considered it
fresh. `touch` on the file restored a correct build and the test passed. Because
this is the second time that class of error has appeared here, the final suite
numbers below were produced from a **fresh worktree of the successor commit with
its own target directory**, so no cached artifact from this session contributes
to them.

## Handoff

The delta is complete against the settled scope: clauses 2, 5, 6, 7 and 8 are
newly enforced, the rest are proved still enforced, all five required
verifications pass, and all eight mutants are killed and reverted. The branch
carries no deployment, no merge, no push, and no Jira mutation.

This seat did not approve or advance any gate.
