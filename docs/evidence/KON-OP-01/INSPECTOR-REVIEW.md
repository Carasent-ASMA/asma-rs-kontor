# KON-OP-01 / ASMA-7870 — inspector gate review

Date: 2026-08-16
Seat: Inspector (independent evaluation; not the builder seat)
Task: `01a0074f-671a-7420-b395-163d160d9792` · TeamRun `01a00760-e384-7ad2-9db9-bcf63d6a1b42`

## Verdict

**FAIL.**

Two deliverables named in the current KON-OP-01 **Implementation** clause, and
three **Acceptance/Verification** clauses that depend on them, are absent from
the tree. The builder disclosed the gap honestly and did not conceal it, but
disclosure is not delivery: against the approved plan at superproject master
`6e30536`, required OP-01 scope is missing. A PASS is not available.

The work that *was* delivered is sound. This is an incomplete ticket, not a
defective one.

## Authority used

Authoritative plan:
`_docs/ai-orchestration/plans/2026-08-14-23-21-plan-kontor-operational-mvp.md`
read from superproject **master `6e30536`**, not from this worktree.

The worktree's own copy is stale at merge-base `4480dae` and states the
*opposite* requirement on the decisive point (see the plan-drift section). Every
judgement below is made against the `6e30536` text.

## Exact commits reviewed

| Item | Value |
| --- | --- |
| Superproject HEAD | `6fdf42a` |
| Superproject branch | `feat/ASMA-7870-kontor-operational-domain` |
| Submodule HEAD | **`f68e3f3`** |
| Integration baseline | `origin/master` = `5e38792` |
| Range reviewed | `5e38792..f68e3f3` — 20 commits, all 2026-08-16 |

Local `master` in the submodule is stale at `7f861a0`; diffing against it
misattributes inherited master work to OP-01. All attribution below uses
`5e38792`.

**OP-01 domain/store code (4):**
`7314721` persist generic topology domain ·
`dedd300` stamp published documents with shareability ·
`597fa26` prove publishing refuses a class nobody chose ·
`b367683` seed the Epic Control Plane and server-owned code help

**Co-mingled hotfix cluster (10):**
`2258517`, `f62702c`, `03abf60`, `45f0638`, `881ed9b`, `184b430`, `7813060`,
`51d103d`, `6f0b6e9`, `6b3e95c` — `fix(paseo|runtime|sessions)`

**Evidence/docs (6):**
`eefedb8`, `c1638f8`, `49ab481`, `94ce942`, `552c444`, `f68e3f3`

## Ownership trace

The four OP-01 code commits touch **only** `kontor-core`, `kontor-profiles` and
`kontor-store` — strictly inside the plan's `Owns` list, and inside the
`OP-01-generic-domain-store-only` scope guard. No runtime projection, `/v1`,
MCP, CLI, Jira/memory cutover or topology mutation was performed by OP-01.
`kontor-teams` was correctly left alone per the `Owns` negative clause.

The `kontor-api`, `kontor-daemon`, `kontor-mcp`, `kontor-runtime` and
`kontor-runtime-paseo` churn visible in a naive `master..HEAD` diff belongs
entirely to the hotfix cluster, which is separately authorized (superproject
`6fdf42a` "Advance operational hotfix", gitlink parked at the cluster tip). It is
**not** charged to OP-01. See P2-4 for the consequence.

## Findings

### P0-1 — `independent_review@1` and `operational_default@1` are not seeded

The current Implementation clause reads: *"Seed only `independent_review@1` and
`operational_default@1` with one remediation round."*

Neither exists anywhere in the tree:

```
$ grep -rn "independent_review\|operational_default" crates/ \
      --include=*.rs --include=*.sql --include=*.json
crates/kontor-profiles/tests/operational_domain.rs:42:fn the_operational_default_is_exactly_the_approved_kind_vocabulary()
```

The single hit is a test function name. The shipped completion-profile ids in
`crates/kontor-profiles/fixtures/mvp-profile-pack.json` are `code`,
`ux-ui-layout`, `research` and their gates. `grep -rn "remediation"` returns only
prose inside a role `responsibility_summary`.

**Impact:** the OP-01 completion/Committee boundary does not exist, so nothing
downstream can pin it.

### P0-2 — Acceptance and Verification clauses that depend on P0-1 are unmet

- Acceptance: *"Test-only two- and five-seat Committee templates pass through the
  same publish boundary and kill a hard-coded-three-seat mutant while only
  `independent_review@1` is seeded."* No Committee template exists in
  `kontor-teams/src` or `kontor-profiles/src`; no two-/five-seat fixture; the
  hard-coded-three-seat mutant is unkilled.
- Implementation: *"Migrate the Foundation three-seat compliance fixture to that
  pinned template without changing its semantics."* Not done — there is no pinned
  template to migrate onto.
- Verification: *"…test-only two-/five-seat Committee templates,
  `operational_default@1`, a custom completion fixture…"* — none present.

### P1-1 — OQ-OP-01-1 was closed by the builder, not by its named authority

`docs/evidence/KON-OP-01/OPEN-QUESTIONS.md:15-49` raises OQ-OP-01-1, declares
**"Closes: LSA (architectural — ownership of seeded specification data)"**, and
then carries a `**CLOSED — the brief was right and the worktree's plan copy was
stale.**` disposition written in the builder's own voice. No LSA closure is
recorded anywhere in the evidence set.

Under OP-REQ-038 the `LSA` closes architectural and product questions; a seat
raises but does not close its own architectural question. By contrast OQ-OP-01-3
correctly records "CLOSED by the TPM", which shows the seat knows the
distinction.

The effective disposition is also not one of the three OP-REQ-038 permits. The
text says the work "needs its own run", which is a `deferred` in substance but
names no concrete reopening trigger. OP-REQ-038 states an open question with no
valid disposition is not a valid end state.

**This is the governance defect, and it is the one that turns P0-1 from "a
sequencing decision" into "an unresolved gate blocker."**

### P1-2 — RELEASE-NOTES frames required scope as out-of-scope

`RELEASE-NOTES.md` "Not in this release" states:

> `independent_review@1` and `operational_default@1`. The amended plan assigns
> these to OP-01, but the amending brief scoped this run to shareability, ECP
> and code help; they are remaining OP-01 work (OQ-OP-01-1).

The first half is accurate and creditable. The second half elevates a run brief
above the approved plan. A brief may sequence work inside a ticket; it does not
narrow that ticket's acceptance. Presented to a reader who does not diff the
plan, this reads as a scope boundary when it is in fact an unmet acceptance
clause. See the plan-drift conclusion below.

### P2-1 — `AdaptiveAdmissionState::validate` is dead code; floor/ceiling never enforced

`crates/kontor-core/src/state.rs:1679` defines
`validate(&self, floor: u32, ceiling: u32)`, documented to reject "a
zero/out-of-range window". **No caller exists** — not in production, not in
tests. `grep -rn "AdaptiveAdmissionState" crates/` returns only construction,
repository ports and row mapping.

The write path instead calls `validate_adaptive_values`
(`crates/kontor-store/src/repository.rs:892`), which receives no floor or
ceiling and checks only `current_window == 0 || clean_observation_streak > 1`.

**Failure scenario:** `create_adaptive_admission_state` with
`current_window: 10_000` against a config whose ceiling is 7 is accepted and
persisted. Nothing refuses it at write time. The scheduler clamps on read
(`AdaptiveWindow::restore`), so the invalid row is survivable — which is why this
is P2 and not P1 — but the documented write-time invariant does not hold.

### P2-2 — `clean_observation_streak` encodes a growth rule the scheduler does not have

`crates/kontor-store/migrations/0023_operational_topology.sql:141` pins
`CHECK (clean_observation_streak BETWEEN 0 AND 1)`, and the domain rejects `> 1`
with the rationale "the two observations Operational requires before growth".

The scheduler has no streak. `AdaptiveWindow`
(`crates/kontor-scheduler/src/model.rs:782`) holds only `current`, and
`observe()` (`:814`) grows by `growth_step` on **every** clean observation:

```
CapacityObservation::Clean => self.current.saturating_add(config.growth_step).min(config.ceiling)
```

Confirmed by the scheduler's own test
`the_adaptive_window_grows_on_clean_observations_and_falls_to_the_floor_under_pressure`
(`crates/kontor-scheduler/tests/ready_batch.rs:1325`): 4→5→6→7 on consecutive
clean observations, no two-observation gate anywhere.

The plan asked to persist *"the existing scheduler's adaptive state"*. This field
persists a rule that exists in no other component, and the `CHECK` makes it a
hard database refusal: whichever ticket later implements a real streak must ship
a migration before it can store the value `2`.

### P2-3 — superproject gitlink does not record the OP-01 work

`git status` in the superproject reports clean. That is misleading:
`.gitmodules` sets `submodule._tools/asma-rs-kontor.ignore = all`.

```
$ git ls-tree HEAD _tools/asma-rs-kontor
160000 commit 6b3e95c...   _tools/asma-rs-kontor
$ git status --short --ignore-submodules=none
 M _tools/asma-rs-kontor
```

The recorded gitlink is `6b3e95c` — the **hotfix** tip — while the submodule is
at `f68e3f3`. Every OP-01 domain commit (`dedd300`, `597fa26`, `b367683`,
`f68e3f3`) is unrecorded in the superproject. Anyone checking out this branch
recursively gets the hotfix without the OP-01 work.

### P2-4 — hotfix cluster is co-mingled on the OP-01 branch

Ten `fix(paseo|runtime|sessions)` commits sit between `7314721` and `dedd300`,
touching five crates outside OP-01's `Owns` list. They appear separately
authorized and I do not charge them to OP-01, but they are inseparable from it
now: merging the OP-01 branch merges the hotfix, and reverting OP-01 would
revert runtime fixes. Ticket-scoped review of either half is no longer possible
from the branch alone.

### P3-1 — OPEN-QUESTIONS header contradicts its own contents

`OPEN-QUESTIONS.md:5` declares `Status: open — raised, not closed here`, while
four of the six entries carry `CLOSED` dispositions in the body. Cosmetic, but
the header is what a scanning reader trusts.

### P3-2 — `master` is red on clippy and stays red

`OQ-OP-01-5` records that `origin/master` at `5e38792` fails
`cargo clippy --workspace --all-targets -- -D warnings`, inherited from
`bbc7e52` (OP-REQ-039), and that the merge commit `181c628` fixes it on this
branch only. Accurate and well-evidenced disclosure. Flagged here so it is not
lost with the ticket: master remains red until this branch merges.

## Acceptance clause map

Clauses are taken verbatim in order from the KON-OP-01 **Acceptance** paragraph
at `6e30536`.

| # | Clause | Status | Evidence |
| --- | --- | --- | --- |
| 1 | Invalid/cyclic parents, missing/duplicate roots, undeclared kinds, invalid capability sets, cardinality violations, duplicate node/seat keys, unknown/duplicate role codes, mutable published specs, free-form roles fail validation | **Partial** | Implemented: `spec.rs:656` `TopologySpec::validate` (one-root, cardinality, cycle/reachability `:779`), `:785` `validate_capabilities`, `:824` `validate_nodes`, `:499` `validate_against` catalog; DB `ux_active_seat_binding_key` (`0023:130`). **Negative-path tests absent** — no test asserts the duplicate-root, cyclic-kind or cardinality refusals |
| 2 | One ECP with distinct LSA/TPM; SA cannot satisfy LSA; LSA/TPM never node kinds; TSW one workspace; ASW≠CSW; Seat never a node kind | **Met** | `operational_domain.rs` `one_epic_control_plane_sits_under_each_epic_workspace`, `control_roles_are_seat_bindings_and_never_topology_kinds`, `the_lead_architect_slot_cannot_be_filled_by_a_plain_architect`, `the_operational_default_is_exactly_the_approved_kind_vocabulary` |
| 3 | A separately published spec may change the ECP roster / declare ECP child kinds without a kernel change; pinned epic unchanged until preview/apply | **Missing** | No alternate-roster fixture; no preview/apply upgrade fixture. Tests only mutate the seeded spec in place |
| 4 | Every published kind and role code has non-empty code-help full name and meaning | **Met** | `every_seeded_code_carries_server_owned_help` (asserts non-empty, non-echo, category, lifecycle) |
| 5 | A separately published **test** spec can declare a different valid kind vocabulary without a kernel code change | **Missing** | No alternate-vocabulary fixture exists. The Verification clause names this explicitly ("an alternate declared-kind fixture proving the kernel does not enumerate Operational kinds"). Data-defined kinds are plausible by construction but unproven by test |
| 6 | Two-/five-seat Committee templates through the same publish boundary kill a three-seat mutant while only `independent_review@1` is seeded | **Missing** | **P0-1 / P0-2.** No Committee template, no fixture, no seeded profile |
| 7 | Tracker bindings optional for a kernel MiniProject; ASMA Epic policy validates Jira cardinality | **Not evidenced** | No test found asserting optional tracker binding or the Epic-policy cardinality check |
| 8 | Adaptive admission state survives restart/export/restore, ignores a replayed observation id, stays separate from topology and Completion Profile | **Met** | `operational_topology.rs::operational_state_survives_restart_and_typed_export`; replay guard `repository.rs:1662`; separate `adaptive_admission_state` table + export row (`backup/export.rs:610`). Caveat P2-1/P2-2 |
| 9 | Old data opens unchanged; export/restore preserves spec document/hash, node, binding, evidence; new writes emit canonical terms | **Met** | `schema_v1.rs::documents_published_before_the_classification_existed_adopt_the_tier_default`; `id.rs:532` TSC→CSW normalization on parse; export round-trip test |
| 10 | Every classifiable record and published document carries immutable write-time shareability + classifier + provenance; unclassified/post-hoc reclassified fails; tier-A refuses; tier-B/C default validates | **Met** | `spec_validation.rs`: `tier_a_operational_state_refuses_classification`, `each_classifiable_tier_has_a_default_so_work_never_stalls`, `an_override_is_attributable_and_a_default_is_not`, `classification_spellings_are_stable_and_closed`; `operational_topology.rs`: `a_published_classification_cannot_be_revised_after_the_fact`, `an_unattributed_override_is_refused_by_the_schema`, `publishing_refuses_a_class_nobody_chose` |
| 11 | Published project-scoped config is classifiable; provider/capacity/cost/credential fields stay tier A regardless of carrier | **Met** | `schema_v1.rs::tier_a_operational_tables_have_nowhere_to_store_a_classification`; `spec.rs:272` `validate_for(tier)`; `documents_that_carry_credentials_or_identity_are_refused_before_sql` |
| 12 | `in_progress` refused once every seat has passed deadline/orphaned/stalled/released, while freshly created seats still admit | **Met (inherited)** | `domain_state.rs:1630` `a_task_cannot_claim_progress_without_an_attached_seat`; `state.rs:214` `evaluate_seat_attachment`. Delivered by `bbc7e52` in the `5e38792` baseline, **not** by OP-01 |
| 13 | Five-seats-never-attached fixture; runtime self-reports `running` but stale activity reads `stalled` | **Met (inherited)** | `domain_state.rs:1717` `an_unattached_seat_becomes_a_finding_once_its_deadline_passes`, `:1762` `an_attached_seat_that_never_showed_activity_is_stalled_not_healthy`, `:1776` `an_orphan_is_an_orphan_however_healthy_its_runtime_looks` |
| 14 | Mutants (unattached seat holds progress / never-observed read as fresh / caller asserts progress without certificate) all die | **Met (inherited)** | Same suite; store-derived `SeatAttachment` certificate gate |

**Score: 7 met, 3 met by inherited baseline work, 1 partial, 3 missing, 1 not
evidenced.** Clauses 3, 5 and 6 are outstanding OP-01 work.

Verification-clause artifacts additionally named in the plan and not found:
generic tree/spec **property** tests for the topology (the `any_cycle_is_refused`
proptest at `spec_validation.rs:564` covers phase graphs, not topology nodes),
the **parent/cardinality truth table**, ECP **alternate-roster** and
**pinned-upgrade** fixtures, and the **custom completion fixture**.

## Conclusion on the plan-drift / scope discrepancy

**Ruling: the current plan governs, and OP-01 is therefore incomplete.**

The facts, established by reading both texts:

- This worktree's plan copy (merge-base `4480dae`) says: *"**Do not seed
  `independent_review@1` or `operational_default@1`**, and do not modify the
  Foundation three-seat fixture… those remain with OP-05, OP-06."*
- Master `6e30536` says: *"**Seed only `independent_review@1` and
  `operational_default@1`** with one remediation round. Migrate the Foundation
  three-seat compliance fixture to that pinned template."*

These are exact opposites, and the builder followed the copy physically present
in its worktree. That is a reasonable thing for a seat to do and the root cause
is a real process defect, correctly identified in OQ-OP-01-6: a worktree that
ships a stale requirements register produces confidently wrong scope decisions.
I endorse that finding.

But the resolution does not follow from it. Once the builder learned the master
copy required seeding — which it did, and recorded in OQ-OP-01-1 — the correct
dispositions were to do the work, or to obtain an `LSA` deferral naming a
concrete reopening trigger. Instead the question was self-closed and the
requirement re-labelled "remaining OP-01 work" in `RELEASE-NOTES.md` on the
authority of a run brief.

**Is this hidden scope reduction?** Not hidden — `OPEN-QUESTIONS.md` states
plainly that the amended plan assigns the work to OP-01 and that it was not done.
That transparency is genuine and I credit it. It **is** scope reduction: the
ticket's acceptance surface was narrowed by a brief that does not outrank the
plan, and the narrowing was ratified by the seat that benefited from it. A brief
may sequence work within a ticket; it cannot discharge that ticket's acceptance
clauses. Against current approved authority OP-01 has not met its Implementation
or Acceptance clauses, and the gate must record that rather than absorb it.

**Plan drift is real and bidirectional** — the plan moved under a running ticket,
and the ticket's view of the plan did not move with it. OQ-OP-01-6's process ask
(refresh `_docs` in the worktree, or mandate reading from the main checkout)
should be actioned by the `TPM` before the next Operational ticket starts; every
seat after this one inherits the same trap.

## Commands and results

Run by me in this worktree. Nothing here is quoted from builder-authored
evidence.

```
# Ground truth — authoritative plan
$ git -C /Users/igor/carasent/asma-modules show 6e30536:_docs/ai-orchestration/plans/\
2026-08-14-23-21-plan-kontor-operational-mvp.md          → 507 lines

# Cleanliness and attribution
$ git status --short                                      → (clean, misleading)
$ git config -f .gitmodules --get-regexp kontor           → ignore = all
$ git status --short --ignore-submodules=none             → " M _tools/asma-rs-kontor"
$ git ls-tree HEAD _tools/asma-rs-kontor                  → 6b3e95c  (submodule at f68e3f3)
$ git -C _tools/asma-rs-kontor status --short -uall       → (clean)
$ git merge-base HEAD origin/master                       → 5e38792
$ git log --no-merges 5e38792..f68e3f3                    → 20 commits
$ git show --stat 7314721 dedd300 597fa26 b367683 -- crates tests
      → kontor-core, kontor-profiles, kontor-store only

# Scope verification
$ grep -rn "independent_review\|operational_default" crates/ \
      --include=*.rs --include=*.sql --include=*.json     → 1 hit (a test fn name)
$ grep -rn "remediation" crates/ --include=*.rs --include=*.json --include=*.sql
      → 1 hit (role responsibility prose)
$ grep -rln "[Cc]ommittee" crates/kontor-teams/src crates/kontor-profiles/src
      → (no matches)

# Finding verification
$ grep -rn "AdaptiveAdmissionState" crates/                → no validate() caller
$ grep -n "CHECK (clean_observation_streak" \
      crates/kontor-store/migrations/0023_operational_topology.sql:141
      → BETWEEN 0 AND 1
$ grep -rn "TopologySpec" crates/*/tests/                  → operational_domain.rs only

# Targeted tests (OP-01's three owned crates)
$ cargo test -p kontor-profiles -p kontor-store -p kontor-core --quiet
      → 61 passed / 0 failed
      → 24 passed / 0 failed
      → 37 passed / 0 failed
      → 122 total, 0 failures

# Full workspace suite
$ set -o pipefail; cargo test --workspace 2>&1 > /tmp/op01-full-suite.log; echo $?
      → CARGO_EXIT=0
      → 1280 passed / 0 failed
```

The 1280-test figure claimed in `RELEASE-NOTES.md` is **independently
confirmed**: my own run reproduces it exactly, at cargo exit code 0.

Both suites are green. **Test success does not mitigate the verdict:** the
missing clauses have no tests precisely because the code they would cover was
never written.

## Confirmation

- **No implementation was changed by this review.** No file under `crates/`,
  `migrations/` or `tests/` was created, edited or deleted by the Inspector seat.
- No Kontor, Paseo or Jira state was mutated. No topology was created. No
  lifecycle transition, gate record or memory write was performed.
- The only artifact produced is this document. It is committed alone; all other
  working-tree and branch content is preserved exactly as received.
- Commands run were read-only inspections plus two `cargo test` invocations,
  which regenerate `docs/evidence/KON-MVP-18/run-*` as OQ-OP-01-3 documents.
  Those regenerated bundles are **not** committed by this review.

## Required to clear the gate

1. Seed `independent_review@1` and `operational_default@1` with one remediation
   round, and migrate the Foundation three-seat compliance fixture onto the
   pinned template without changing its semantics (P0-1, P0-2, clause 6).
2. Add the alternate declared-kind fixture and the ECP alternate-roster /
   pinned-upgrade fixtures (clauses 3 and 5).
3. Obtain a real `LSA` disposition on OQ-OP-01-1 — `resolved` by doing the work,
   or `deferred` naming a concrete reopening trigger. Self-closure does not
   satisfy OP-REQ-038 (P1-1).
4. Correct `RELEASE-NOTES.md` so the two profile packs read as unmet OP-01
   acceptance, not as out-of-scope (P1-2).
5. Add negative-path tests for the topology tree refusals already implemented
   (clause 1), and evidence clause 7.
6. Advance the superproject gitlink to the OP-01 tip once merged (P2-3).

P2-1 and P2-2 are defects in delivered code and should be triaged by the `LSA`;
neither blocks this gate on its own.
