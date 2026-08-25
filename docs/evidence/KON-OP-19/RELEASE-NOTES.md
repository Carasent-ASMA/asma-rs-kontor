VERDICT: PASS

# KON-OP-19 / ASMA-7967 — release gate

> **Seat:** architect (release-gate, evidence `release-notes`)
> **Ticket:** KON-OP-19 / ASMA-7967 — "Make native container and seat naming a configurable deterministic template"
> **Epic:** ASMA-7869
> **Date:** 2026-08-22
> **Category:** report
> **Records no Kontor gate.** The LSA records the gate citing this verdict.

This gate is **post-deployment**. All four PRs merged to `origin/master` unreviewed, and the
live realm daemon already runs a build containing them. This is therefore not a
ship/don't-ship decision — that decision was taken by merging. It is a judgement on what is
already running, what an operator must know about it, and what must be true before the next
naming repair is run in anger.

Upstream gates, both read in full and not re-derived here:

* **code-review-gate PASSED** — `~/.cache/asma-7869-evidence/KON-OP-19-REVIEW-NOTES.md`
  (11 findings: 3xP2 F1/F3/F4, 8xP3; none blocking).
* **qa-gate PASSED** — `~/.cache/asma-7869-evidence/KON-OP-19-QA-REPORT.md`
  (pinned `df64004`: 22/22 packages EXIT=0; `fmt`, `clippy --all-targets`, `clippy -D warnings` all EXIT=0).

Everything below was verified independently in the clean pinned worktree
`~/.cache/op19-qa/tree` @ `df64004` (`git status --porcelain` clean but for one untracked
test artifact directory), and — where the claim concerns what is true *today* — against
`origin/master`. Nothing in the shared checkout `_tools/asma-rs-kontor` was committed,
staged, branched, stashed, reset or modified.

---

## 1. Release identity

**The release is four commits, not a range.** This is the first thing an operator or a future
bisect needs, because the obvious range is wrong.

| PR | Merge commit | Own commit | Subject |
| --- | --- | --- | --- |
| #67 | `c528e9b` | `a18221b` | feat(topology): Render native names from pinned specification |
| #68 | `0c58e72` | `1defdb2` | fix(topology): Tolerate sparse native-name seats |
| #69 | `367a711` | `4ec7ab1` | fix(ASMA-7967): classify missing Paseo seats as stale |
| #70 | `df64004` | `df64004` (squash — merge and content are the same commit) | fix: Refresh hosted seat provider session on native-name repair |

* **Release tip:** `df64004f901ff3edca1c10191939e9d6ce351b53`.
* **The union range `c528e9b~1..df64004` contains 25 commits, of which only 4 are OP-19.**
  The remaining 21 are ASMA-7877 / OP-08 ("operational control surfaces", PR #64, merged as
  `527cc16`, with `6f884a0`, `d310cdc`, `d783ccd` and others interleaved *between* OP-19's own
  merges). OP-19 is therefore **not a contiguous, separable range of master**. Any future
  "revert OP-19" that operates on the range reverts OP-08 as well.
* **Change set size** (each commit against its own first parent):

  | Commit | Files | +/- |
  | --- | --- | --- |
  | `a18221b` | 49 | +4845 / -589 |
  | `1defdb2` | 7 | +214 / -16 |
  | `4ec7ab1` | 3 | +46 / -3 |
  | `df64004` | 6 | +120 / -15 |

  Effectively a single large feature commit plus three small corrective follow-ups.

* **Schema:** OP-19 owns exactly one migration, `0047_configurable_native_names.sql`.
  `SCHEMA_VERSION` at `df64004` is **47**.
* **The live realm is at 48, and 48 is not OP-19's.** Migration 48 is
  `0048_provider_quota_states.sql`, introduced by OP-13 quota routing (`1370ba5`). Verified:
  it is the only migration file added between `df64004` and `origin/master`, and
  `origin/master`'s `SCHEMA_VERSION` is 48. This matters entirely for rollback (§5).

### Master has moved past this release

`origin/master` is now `7dc6212`, **7 commits ahead** of `df64004`. Relevant to this gate:

* `2d2bedd` — **"ASMA-7869 Serve API during startup reconciliation (#74)"** is **already
  merged**, not merely a branch. The brief described it as an in-flight branch; it landed.
  The code review's containment argument still stands and I re-checked its basis: #69's
  reclassification cannot reach startup reconciliation because both restore call sites
  discard the error *variant* (`let Ok(agent) = ... else { continue }`). Nothing in this
  verdict judges #74's own correctness — it was not reviewed by this gate.
* Post-OP-19 drift on the OP-19 surface is real but additive: `spec.rs` +45,
  `spec_validation.rs` +43, `applications.rs` +236, `adapter.rs` +87, `repository.rs` +164.
  I diffed each to confirm **none of the three findings below has been fixed** (see §3).

---

## 2. What shipped, and what an operator must know

Four operator-visible behaviour changes, plus one new surface.

**New surface:** `kontor_native_names_preview` and `kontor_native_names_apply` (added by
`a18221b`, `crates/kontor-mcp/src/registry.rs:3056`, `:3084`). This is the operator's entry
point to everything below.

### 2.1 Deterministic naming rendered from a pinned spec

Names now come from `NativeNameTemplate::render` against the node's **own pinned**
`(spec_id, version)`, not the latest published spec. Missing token = refusal, no fallback,
no derivation. The pre-change catch-all that appended the internal node id is gone.

*What the operator must know:* **a name that will not render is now a refusal, not a
degraded-but-present name.** Under the old regime every container got *a* name. Under this
one a node whose template requests a token the epic never declared has no name at all and
reports `rename_pending` indefinitely. Silence is now a real state.

### 2.2 The naming tokens are permanent and uncorrectable — R-1, new at this gate

**This is the most consequential operator-facing fact in the release, and neither upstream
gate records it.** Migration 0047 creates `epic_native_name_tokens` and `task_ai_short_names`
with `BEFORE UPDATE` and `BEFORE DELETE` triggers that `RAISE(ABORT, ...)`:

```
CREATE TRIGGER epic_native_name_tokens_are_immutable
BEFORE UPDATE ON epic_native_name_tokens
BEGIN SELECT RAISE(ABORT, 'epic native-name tokens are immutable'); END;

CREATE TRIGGER epic_native_name_tokens_are_permanent
BEFORE DELETE ON epic_native_name_tokens
BEGIN SELECT RAISE(ABORT, 'epic native-name tokens are permanent'); END;
```

(`crates/kontor-store/migrations/0047_configurable_native_names.sql`, and the identical pair
on `task_ai_short_names`.) I confirmed the application layer matches the triggers: a repo-wide
search over `crates/*/src/` finds **only `INSERT` and `SELECT`** against both tables. There
is no correction path in code and none at the DB either.

Consequences an operator must accept before declaring a token:

1. **`KONTOR_BACKLOG_CODE` is write-once per epic.** A second declaration with a *different*
   code refuses loudly — `"the epic already has a different Kontor backlog code"`
   (`crates/kontor-store/src/graph.rs:1662-1666`). Good: fail-closed, visible.
2. **A late `AI_SHORT_NAME` is silently dropped.** The conflict match keys on
   `kontor_backlog_code` alone (`graph.rs:1658-1689`). If the epic already stores a backlog
   code and a later declaration re-sends that same code while adding an `AI_SHORT_NAME` that
   was not stored, control falls to the `_ => {}` arm: **nothing is written and nothing is
   refused.** Combined with §2.1's fail-closed render, an epic in this state can never satisfy
   a template requesting `AI_SHORT_NAME`, and its seats stay `rename_pending` **permanently**.
   This is a latent trap, not an observed incident — I did not inspect live rows and do not
   claim any epic is in this state.
3. **`AI_SHORT_NAME` must be exactly two words.** The DB `CHECK` requires 3-64 chars, trimmed,
   one interior space and no second space, and no `U+2022`/`U+00B7`. A one-word short name is
   rejected by SQLite, not by a friendly validator.

*Operator action:* treat a token declaration as irreversible. Declare `KONTOR_BACKLOG_CODE`
and `AI_SHORT_NAME` **together, in the first declaration**, and read the epic back to confirm
both landed before materializing anything that depends on them.

### 2.3 Sparse-seat tolerance

A seat the runtime cannot read no longer refuses the whole epic's repair. It degrades to a
census entry with `capability: "rename_pending"` and `observed_title: null`, and the rest of
the epic is repaired.

*What the operator must know:* **`would_change: false` does not mean converged.** A
`rename_pending` target reports `would_change: false` while being precisely the thing that did
*not* converge. Read `capability`, never `would_change` alone. The preview→apply hash covers
`capability`, `observed_title`, `provider_session_id` and `would_change` with no
`skip_serializing_if`, so a seat that flips between preview and apply forces a
`RevisionConflict` on the **whole plan** rather than a half-apply — this is the property that
makes the tolerance safe, and it is verified by both upstream gates.

### 2.4 Stale-vs-drift seat classification

A `null` agent from Paseo now means "the persisted binding is stale"; an answer carrying a
*different* agent id still means correlation drift. Only `prepare_native_names` can observe
the difference — `ApiError::from_runtime` folds `StaleBinding`, `CorrelationFailed` and
`SessionAlreadyBound` into one code with one message, and the daemon never matches
`CorrelationFailed` anywhere.

*What the operator must know:* this widened what a naming repair *tolerates*. It did not
widen what any other surface tolerates, and it is invisible everywhere else.

### 2.5 Provider-session refresh on native-name repair

A hosted seat's stored `provider_session_id` is now refreshed from the observation taken
during a naming repair. Route and native identity are still immutable — a mismatch is still
`RepositoryError::Conflict` "a persistent seat cannot change its route or native identity".

*What the operator must know:* **a naming repair now writes to a field that is not a name.**
That is the surprise in this release. See F1 in §3.1 for the condition under which it writes
the wrong thing.

---

## 3. The three findings that need release treatment

All three re-verified against `origin/master` today. **None has been fixed.**

### 3.1 F1/F6 — `Some -> None` provider-session erasure: ship as-is, gate the *operation*

**Decision: it ships as-is — and it already has. The fix is not a release blocker; it is a
precondition on the next hosted-seat repair.**

The reviewer called this a one-line fix worth making "before the next hosted-seat repair runs
in anger", and that framing is exactly right — the risk is not carried by the deployed binary
sitting idle, it is carried by *running the tool*. `kontor_native_names_apply` is operator-
triggered. Nothing erases a handle until someone runs a repair. So the correct release
treatment is not to block a release that already happened, but to bind the fix to the
operation:

> **Do not run `kontor_native_names_apply` against an epic containing hosted seats until the
> `is_some()` guard and its covering test have landed.** Preview is safe and unrestricted;
> `native_names:preview` performs no write. Container-only repairs are unaffected.

Why this is not stronger: the blast radius is **evidence, not routing**. The code review
traced every use of the stored field and found no launch, resume, or message path reads it.
Nothing breaks; observability is lost. Why it is not weaker: the erasure is *silent*, the
value is durable, and before #70 the same combination was a loud `RepositoryError::Conflict`
— so this release converted a refusal into a quiet overwrite.

Verified still open on master:

* Write site unguarded — `hosted.provider_session_id = outcome.provider_session_id.clone();`
  at `origin/master:crates/kontor-daemon/src/applications.rs:10148` (line moved from `:9986`
  at `df64004`; the expression is unchanged). No `is_some()`, no `.or(persisted)`.
* Still exactly one `set_seat_provider_session` call site outside `fake.rs`
  (`crates/kontor-daemon/tests/loopback_api.rs:16190`), still passing `Some(...)`. **No test
  passes `None`.**

**Fix and test together, in one change.** The QA gate's observation is the operative one: the
one-line fix could be applied and later reverted with the suite fully green. A fix without the
test does not stay fixed. `ScriptedFakeRuntime::set_seat_provider_session` already takes an
`Option`, so the test is one line.

### 3.2 F4 — the hardening is pinned open by a test; that edit is NOT a regression

**Recorded plainly here so it is not misread later, which is the whole point of this entry.**

The recommended hardening — require at least one token segment when `projection_capabilities`
contains `native_root`, mirroring the rule `validate` already applies to `session_host` —
**cannot be applied without editing an existing passing test.** Whoever does it will produce a
diff that edits a green fixture, and that will look like a test regression to a reviewer, to
CI, and to a future bisect. It is not one.

The fixture, confirmed **byte-identical on `origin/master` today**
(`crates/kontor-core/tests/spec_validation.rs`):

* `:86-93` — the `minimal_native_topology()` fixture declares root kind `PSW` with
  `"projection_capabilities": ["native_root", "session_host"]` and a bare-literal container
  `"name_template": {"segments": [{"kind": "literal", "value": "Project Session Workspace"}]}`.
* `:106` — `topology_naming_requires_typed_container_and_hosted_seat_templates` asserts this
  fixture **validates**.

The only post-OP-19 change to that file is an unrelated quota test
(`only_an_exhausted_allowance_recovers_on_a_clock`); the fixture and the assertion are
untouched.

> **For the record:** the correct fix changes `minimal_native_topology()` so its `native_root`
> container template carries a token segment. The resulting diff to
> `spec_validation.rs:86-93` is **the intended, required part of the fix**, not collateral
> damage and not a weakened test. A reviewer who asks "why is this fix editing a passing
> test?" should be pointed at this paragraph. The bundled v47 spec's `PSW` is itself a
> `native_root` with the single literal `"Project Session Workspace"`
> (`crates/kontor-store/src/migrations.rs:605`), so the shipped spec will need the same
> treatment.

Release severity: **latent arming, not a live defect.** The code review could not construct a
firing path, and neither can I: the Paseo adapter keys its project binding by its single
configured `mini_project_id`, and `ESW` — the native root actually reached through
`ensure_container` — renders uniquely per epic. The exposure is that this ticket made
templates *operator-configurable*, so a future revision may legally put a bare literal on a
`native_root`, and Paseo's name-keyed adoption adopts an exactly-one display-name match rather
than refusing it. **Operator action:** until the validation rule exists, do not publish a
topology revision whose `native_root` container template is a bare literal. Nothing will stop
you.

### 3.3 F5 — the lifecycle skip is wholly unguarded

**Re-verified today, and the result is stark:** `git grep` for
`TopologyLifecycle::Retired|TopologyLifecycle::Archived` across **every** test directory in
the workspace on `origin/master` returns **zero hits**. No test anywhere constructs a
`Retired` or `Archived` seat. The skip at `applications.rs:4727` could be deleted and the
entire suite would stay green.

Why this one earns release treatment rather than a backlog line: it is the guard the code
review's judgement 3 **leans on**. The reasoning that a dead archived seat no longer refuses a
whole epic's naming repair rests entirely on this `continue`. So the most load-bearing guard
in the release's own risk argument is also its least tested. That asymmetry is the finding.

It is also **silent**: skipped seats are omitted from the census entirely rather than reported.
An operator running a repair on an epic holding archived seats gets no indication those seats
exist and were deliberately not touched.

**Operator action:** after any repair, do not read a clean census as "every seat in this epic
is correctly named". It means "every *Active* seat is". To see the rest, inspect the topology
directly. **Follow-up:** the covering test should assert both halves — that a `Retired`/
`Archived` seat is skipped, *and* that an `Active` sibling in the same epic is still repaired.

---

## 4. The live deadlock — judgement

**The situation.** A seat whose native Paseo session is archived and un-reloadable reaches no
runtime-observed terminal state, so `runtime_settle` returns `unknown` forever,
`runtime_abandon` refuses, `seat_replace` refuses on `slots.latest_closed`, and `complete_task`
refuses with *"a declared role slot has neither ended nor settled its final turn"*.

### 4.1 I agree that #69 is NEUTRAL — and I agree for a stronger reason than the review gave

The code review judged #69's stale-seat classification **NEUTRAL** for this deadlock. **I
agree.** The review's argument is that `retire_session` -> `fetch_agent_with_archive` ->
`fetch_agent` produced `CorrelationFailed` before #69 and `StaleBinding` after, and
`ApiError::from_runtime` collapses both to an identical refusal, so the caller cannot tell the
difference. That is correct and I re-confirmed the fold.

But it understates the case, because it argues from one call path. The stronger statement is
structural: **none of the four refusals in this deadlock is reachable from any code #67-#70
touched.** I traced each to its own site:

| Refusal | Site | Touched by OP-19? |
| --- | --- | --- |
| `seat_replace` on `slots.latest_closed` | `crates/kontor-daemon/src/applications.rs:15836` | No |
| `complete_task` "neither ended nor settled its final turn" | `crates/kontor-daemon/src/applications.rs:1099` (in `certify_team`) | No |
| `runtime_settle` -> `unknown` | runtime observation, no terminal evidence | No |
| `runtime_abandon` refusal | terminal-evidence gate | No |

OP-19's entire daemon-side footprint is `prepare_native_names` and the apply path. The
deadlock lives in team-closure certification and slot replacement. They do not intersect.
**Neutral is right, and it is neutral by construction, not by coincidence.**

### 4.2 Does OP-19 make recovery easier or harder? — Marginally easier, and never harder

**Easier, in one specific and real way.** Before #68, a seat the runtime could not read
refused the *whole* epic's naming repair. After #68 it degrades to `rename_pending` and the
epic's other seats and containers are still repaired. So the presence of one dead archived
seat no longer blocks naming work across the rest of the epic. That is a genuine reduction in
blast radius while the deadlocked seat is being dealt with.

**Harder: no.** I looked specifically for a way this release could tighten the deadlock — a
new precondition, a new refusal, a new write that a recovery path would trip over — and found
none. The one new write outside naming is F1's `provider_session_id` refresh, and no launch,
resume, or message path reads that field, so it cannot gate a recovery.

Two honest qualifications, because "easier" should not be oversold:

1. The relief is to **naming**, not to the deadlock. `seat_replace` and `complete_task` are
   exactly as wedged as before.
2. The relief depends on the `Active`-lifecycle skip — **the guard F5 shows is wholly
   untested** (§3.3). The one place OP-19 helps this deadlock is guarded by the one guard no
   test covers. That is worth stating plainly rather than claiming a clean win.

### 4.3 The recovery lever, which is the answer the critical path actually needs

Neither upstream gate identified an exit. There is one, it is designed for exactly this, and
it predates OP-19 entirely (migration `0020_role_slot_waivers.sql`): **`kontor_role_slot_waive`**.

The `complete_task` refusal is **conditional on there being no waiver**. In `certify_team`
(`crates/kontor-daemon/src/applications.rs:1094-1109`):

```rust
let accounted = self.settled_slots(project_id, team_run_id)?;
if waivers.is_empty() {
    return match slots.certify_from_settled_turns(&accounted, &waivers) {
        Ok(certificate) => Ok(Ok(certificate)),
        Err(_) => Ok(Err(
            "a declared role slot has neither ended nor settled its final turn",
        )),
    };
}
// ... otherwise the disposition basis
let dispositions = self.slot_dispositions(project_id, team_run_id, &accounted)?;
match slots.certify_from_dispositions(&dispositions, &waivers) { ... }
```

**The exact string the realm is hitting tonight is emitted only from the `waivers.is_empty()`
branch.** Record an authorized waiver on the wedged slot and closure is evaluated on the
disposition basis instead, which does not require any runtime-observed terminal state.

I verified the whole chain rather than inferring it:

* **The waiver write never consults the runtime.** `waive_role_slot`
  (`crates/kontor-daemon/src/applications.rs:15218`) requires only: admin capability,
  non-empty evidence, an existing team run, the slot being declared by the pinned template,
  and `expected_team_revision`. It touches no session, no binding, no Paseo. A dead seat
  cannot block it.
* **A bound-but-dead seat still yields a waived disposition.** `slot_dispositions`
  (`:1188-1219`) builds `SlotDisposition::WaivedUnbound` purely from persisted waiver rows and
  **never inspects the binding**. `WaivedUnbound` names the disposition class — no settled
  turn — not a requirement that the seat be unbound. This was the detail most likely to
  invalidate the lever, and it does not.
* **The certificate is real.** `certify_from_dispositions`
  (`crates/kontor-teams/src/run.rs:1764`) accepts `WaivedUnbound` backed by an authorized
  waiver and returns `TerminalOutcome::Succeeded` with basis `RoleSlotDispositions`.

**Two preconditions the operator must check first — the lever is not unconditional:**

1. **The pinned team template must declare a `waiver_policy` for that slot.** If it is `None`,
   `validated_waivers` (`crates/kontor-teams/src/run.rs:1534`) refuses with
   *"the template does not allow this role slot to be waived"*. The template is **frozen** at
   the team run and cannot be amended retroactively. **Check this before anything else** — if
   it is absent, the waiver route is closed and the deadlock needs a different answer.
2. **The waiver must satisfy that policy**: `authorized_by_role` must appear in
   `policy.authorized_roles`, and the evidence must cite **every** entry in
   `policy.required_evidence`.

And one consequence to accept deliberately: **once any waiver exists on a team run, the
disposition basis governs, and every *other* declared slot must then be either settled or
waived** — `certify_from_dispositions` refuses on a missing disposition with *"a declared role
slot is neither settled nor waived"*. A waiver is not a local patch to one slot; it changes
the closure basis for the whole team run. The stronger terminal-runs basis is still attempted
first, so a team where every run genuinely ended is unaffected.

**Recommendation for the critical path:** read the wedged slot's `waiver_policy` out of the
pinned template. If it declares one, `kontor_role_slot_waive` is the designed exit and needs
no code change, no deploy, and no fix from OP-19. If it does not, that absence — not anything
in this release — is the real blocker, and it should be escalated as its own ticket.

---

## 5. Rollback — what it would and would not achieve

**Rolling back OP-19 is not available as an operational lever.** Stated plainly rather than
hedged.

### 5.1 A binary rollback alone cannot start

The live realm database is at **schema 48**. Any binary predating `a18221b` carries
`SCHEMA_VERSION <= 46`, and `migrate` refuses outright:

```
/// * `> SCHEMA_VERSION` — refuse. A newer schema is never downgraded, truncated
///   or guessed at.
if version > SCHEMA_VERSION {
    return Err(StoreError::DatabaseTooNew { found: version, expected: SCHEMA_VERSION });
}
```

(`crates/kontor-store/src/migrations.rs:274-290`.) **There are no down-migrations** — the
dispatch table is index-addressed, so "a migration can only ever be appended" (`:65-72`). A
rolled-back daemon does not start with a degraded feature set; it does not start at all.

### 5.2 The change set is not separably revertable

Reverting OP-19's four commits from master would also have to contend with:

* **The 21 interleaved ASMA-7877 / OP-08 commits** in the same range (§1). A range revert
  takes OP-08 with it.
* **Migration 0047's tables carry permanent data** (§2.2). `epic_native_name_tokens` and
  `task_ai_short_names` are protected by `BEFORE DELETE ... RAISE(ABORT)` triggers. A revert
  would have to drop the tables outright; it cannot empty them.
* **Schema 48 sits on top of 47.** Removing 47 while retaining 48 is not expressible in an
  append-only, index-addressed migration list.
* **Seven commits of subsequent work** now sit on `df64004`, including `2d2bedd` (#74,
  startup reconciliation) which is itself ASMA-7869 work on an adjacent surface.

### 5.3 What rollback *would* achieve, if forced

Only via **restore from backup to a pre-47 snapshot**, accepting the loss of every write since.
That would remove F1's erasure risk, F4's latent arming, and the permanent-token trap — and it
would simultaneously discard OP-08's operational control surfaces, OP-13's quota routing and
#74's startup reconciliation, and reinstate the old naming regime whose container fallback
appended internal node ids. `crates/kontor-store/src/backup/export.rs:670,677` does export both
new token tables, so a snapshot taken now is complete with respect to them.

**Not proportionate.** No finding in this release produces a wrong name, a lost identity, or a
wrong model route. The residual risks are one silent evidence-field overwrite gated behind an
operator-triggered tool, one latent misconfiguration that requires someone to publish a
bare-literal `native_root`, and one untested-but-correct guard.

### 5.4 The realistic remediation path

**Forward-fix only.** In priority order:

1. F1's `is_some()` guard **plus** its `None` test, in one change — before the next hosted-seat
   repair (§3.1).
2. F5's lifecycle-skip test, asserting skip *and* sibling repair (§3.3).
3. F4's `native_root` validation rule, editing `spec_validation.rs:86-93` **deliberately and
   with §3.2 cited in the commit message**, plus the bundled v47 `PSW` template.
4. R-1: decide whether a late `AI_SHORT_NAME` should refuse rather than silently no-op
   (§2.2). Given the tokens are permanent, a silent drop is the wrong default.

---

## 6. Verdict

**VERDICT: PASS**

Both upstream gates passed on real exit codes against a clean pinned tree, and I confirmed
their central claims rather than inheriting them. The ticket's core contract holds: names are
rendered from the node's own pinned specification, fail closed on any missing token, and
derive nothing — and that contract is genuinely tested. `preview_hash` covering the full
target shape is what makes the sparse-seat tolerance safe rather than merely convenient. #69
is contained to a single observation point. #70 preserves route and identity as claimed.

I want to be explicit that this is **not** a pass by default because rollback is unavailable.
Unavailable rollback is a fact I record in §5, not a reason. The release passes on its merits:
no finding produces a wrong name, a lost identity, or a wrong model route; the deficits are in
test coverage of the periphery and in one silent write whose blast radius is evidence rather
than behaviour. Merging four PRs unreviewed was the wrong process, and the post-hoc review
found real things — but what it found does not amount to a release that should be pulled.

Passing **with these conditions**, which are operator obligations, not suggestions:

1. **Do not run `kontor_native_names_apply` against epics with hosted seats** until F1's guard
   and its test land together (§3.1).
2. **Do not publish a topology revision with a bare-literal `native_root` container template**
   until F4's validation rule exists (§3.2).
3. **Declare `KONTOR_BACKLOG_CODE` and `AI_SHORT_NAME` together on first declaration**, and
   read back to confirm. They can never be corrected (§2.2, R-1).
4. **Read `capability`, not `would_change`**, and do not read a clean census as full coverage —
   non-`Active` seats are skipped silently (§2.3, §3.3).

On the epic's critical path: I **agree** #69 is NEUTRAL for the deadlock, by construction —
none of the four refusals is reachable from code this release touched. OP-19 makes recovery
**marginally easier and never harder**, and the relief is to naming rather than to the wedge
itself. The actual exit is `kontor_role_slot_waive`, which predates this release and requires
no fix from it — **conditional on the pinned team template declaring a `waiver_policy` for the
wedged slot.** That is the first thing to check (§4.3).

*This report records no Kontor gate. The LSA records the gate citing this verdict.*
